use std::io::{self, Read, Write as _};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};

use crate::agent_provider::builtin_provider_adapter;
use crate::agents::{
    AgentContinuation, AgentExecutionOptions, AgentTokenUsage, apply_invocation_environment,
    apply_noninteractive_agent_environment, command_args_for_invocation_with_options,
    render_invocation_diagnostics, resolve_agent_invocation_for_planning,
    validate_invocation_command_surface,
};
use crate::build_dashboard::{BuildDashboardOptions, run_build_dashboard};
use crate::cli::{BuildArgs, RunAgentArgs};
use crate::config::{AGENT_ROUTE_AGENTS_BUILD, AppConfig, PlanningMeta, PromptTransport};
use crate::fs::{canonicalize_existing_dir, ticket_workspace_root};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn sigint_handler(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

/// Installs a SIGINT handler that interrupts only the active build subprocess.
fn install_sigint_handler() {
    // SAFETY: the process-global SIGINT handler only flips an atomic flag, which is signal-safe.
    unsafe {
        libc::signal(
            libc::SIGINT,
            sigint_handler as *const () as libc::sighandler_t,
        );
    }
}

#[derive(Debug, Clone, Copy)]
enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug)]
struct OutputChunk {
    stream: OutputStream,
    text: String,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct BuildRunSummary {
    pub(crate) usage: Option<AgentTokenUsage>,
    pub(crate) resumed_turns: u32,
}

#[derive(Debug, Default)]
struct BuildRunOutcome {
    usage: Option<AgentTokenUsage>,
    continuation: Option<AgentContinuation>,
    queued_prompts: Vec<String>,
}

/// In-memory session state for the interactive build loop.
struct BuildSession {
    workspace_dir: PathBuf,
    config: AppConfig,
    planning_meta: PlanningMeta,
    run_count: u32,
    queued_prompts: Vec<String>,
}

/// Runs the `agents build` command.
///
/// Resolves the workspace directory from the provided ticket ID or explicit `--dir` path,
/// validates that the target is a git repository, resolves the provider/model/reasoning settings
/// through the `agents.build` route, and either launches the interactive dashboard or runs one
/// headless agent prompt inside that workspace when `--no-interactive` is used.
///
/// Returns an error when workspace resolution fails, the workspace is not a git repository,
/// `--no-interactive` is used without an initial prompt, provider resolution fails, or a
/// provider subprocess exits unsuccessfully.
pub async fn run_build(args: &BuildArgs) -> Result<()> {
    let source_root = canonicalize_existing_dir(&args.root)?;
    let config = AppConfig::load()?;
    let planning_meta = PlanningMeta::load(&args.root).unwrap_or_default();

    if !args.no_interactive {
        let (initial_workspace, effective_prompt) =
            resolve_interactive_workspace_and_prompt(args, &source_root)?;
        let initial_workspace = match initial_workspace {
            Some(workspace) => Some(validate_git_repo(&workspace)?),
            None => None,
        };
        return run_build_dashboard(BuildDashboardOptions {
            source_root,
            workspace_root: ticket_workspace_root(&canonicalize_existing_dir(&args.root)?)?,
            initial_workspace,
            config,
            planning_meta,
            args: args.clone(),
            initial_prompt: effective_prompt,
        });
    }

    let (workspace_dir, prompt) = resolve_noninteractive_workspace_and_prompt(args, &source_root)?;
    let workspace_dir = validate_git_repo(&workspace_dir)?;
    let (_input_tx, input_rx) = mpsc::channel::<String>();

    let mut session = BuildSession {
        workspace_dir,
        config,
        planning_meta,
        run_count: 0,
        queued_prompts: Vec::new(),
    };
    session.run_count = 1;
    INTERRUPTED.store(false, Ordering::SeqCst);
    install_sigint_handler();

    let full_prompt = apply_deferred_queue(prompt, &mut session.queued_prompts);
    let invocation =
        resolve_build_invocation(&session.config, &session.planning_meta, args, &full_prompt)?;

    eprintln!(
        "--- Run #{} | {} | {} ---",
        session.run_count,
        session.workspace_dir.display(),
        provider_display(&invocation.agent, invocation.model.as_deref()),
    );

    let result = execute_build_agent(&mut session, args, &full_prompt, &input_rx);
    match &result {
        Ok(summary) => {
            eprintln!(
                "Completion summary: status=success, {}",
                render_summary(summary)
            );
            eprintln!(
                "--- Run #{} complete ({}) ---",
                session.run_count,
                render_summary(summary)
            );
        }
        Err(_) if INTERRUPTED.load(Ordering::SeqCst) => {
            eprintln!("Completion summary: status=interrupted");
            eprintln!("--- Run #{} interrupted ---", session.run_count);
        }
        Err(error) => {
            eprintln!("Completion summary: status=failure");
            eprintln!("--- Run #{} failed: {error} ---", session.run_count);
        }
    }

    match result {
        Ok(_) => Ok(()),
        Err(_) if INTERRUPTED.load(Ordering::SeqCst) => Ok(()),
        Err(error) => Err(error),
    }
}

fn resolve_interactive_workspace_and_prompt(
    args: &BuildArgs,
    source_root: &Path,
) -> Result<(Option<PathBuf>, Option<String>)> {
    if let Some(dir) = &args.dir {
        let prompt = match args.positionals.as_slice() {
            [] => None,
            [prompt] => Some(prompt.clone()),
            [_, ..] => bail!("`agents build --dir` accepts at most one positional prompt"),
        };
        Ok((Some(dir.clone()), prompt))
    } else {
        let (workspace, prompt) = match args.positionals.as_slice() {
            [] => return Ok((None, None)),
            [workspace] => (workspace, None),
            [workspace, prompt] => (workspace, Some(prompt.clone())),
            [_, _, ..] => bail!("`agents build` accepts at most two positional arguments"),
        };
        let workspace_path = PathBuf::from(workspace);
        if path_like_workspace_arg(&workspace_path) || workspace_path.exists() {
            Ok((Some(workspace_path), prompt))
        } else {
            Ok((
                Some(ticket_workspace_root(source_root)?.join(workspace)),
                prompt,
            ))
        }
    }
}

fn resolve_noninteractive_workspace_and_prompt(
    args: &BuildArgs,
    source_root: &Path,
) -> Result<(PathBuf, String)> {
    let (workspace, prompt) = resolve_interactive_workspace_and_prompt(args, source_root)?;
    let workspace = workspace
        .ok_or_else(|| anyhow!("workspace identifier is required when `--dir` is not provided"))?;
    let prompt = prompt.ok_or_else(|| anyhow!("`--no-interactive` requires a prompt"))?;
    Ok((workspace, prompt))
}

fn validate_git_repo(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        bail!("workspace directory does not exist: `{}`", path.display());
    }
    if !path.is_dir() {
        bail!("workspace path is not a directory: `{}`", path.display());
    }
    let canonical = canonicalize_existing_dir(path)
        .with_context(|| format!("failed to resolve workspace `{}`", path.display()))?;
    let output = Command::new("git")
        .arg("-C")
        .arg(&canonical)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .with_context(|| {
            format!(
                "failed to inspect git metadata for workspace `{}`",
                canonical.display()
            )
        })?;
    if !output.status.success() {
        bail!(
            "workspace directory is not a git repository: `{}`",
            canonical.display()
        );
    }
    let is_inside_work_tree = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if is_inside_work_tree != "true" {
        bail!(
            "workspace directory is not a git repository: `{}`",
            canonical.display()
        );
    }
    Ok(canonical)
}

fn path_like_workspace_arg(path: &Path) -> bool {
    path.is_absolute()
        || path.components().count() > 1
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
}

/// Formats the resolved provider/model label for build status output.
pub(crate) fn provider_display(agent: &str, model: Option<&str>) -> String {
    match model {
        Some(model) => format!("{agent} ({model})"),
        None => agent.to_string(),
    }
}

/// Renders a concise completion summary for one or more build turns.
pub(crate) fn render_summary(summary: &BuildRunSummary) -> String {
    let mut parts = Vec::new();
    match &summary.usage {
        Some(usage) => {
            let input = usage
                .input
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".to_string());
            let output = usage
                .output
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".to_string());
            let total = add_optional_u64(usage.input, usage.output)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".to_string());
            parts.push(format!("tokens in={input} out={output} total={total}"));
        }
        None => parts.push("tokens unavailable".to_string()),
    }
    if summary.resumed_turns > 0 {
        parts.push(format!("resumed {} time(s)", summary.resumed_turns));
    }
    parts.join(", ")
}

/// Adds two optional token counts, preserving whichever side is present.
pub(crate) fn add_optional_u64(lhs: Option<u64>, rhs: Option<u64>) -> Option<u64> {
    match (lhs, rhs) {
        (Some(lhs), Some(rhs)) => Some(lhs + rhs),
        (Some(lhs), None) => Some(lhs),
        (None, Some(rhs)) => Some(rhs),
        (None, None) => None,
    }
}

fn merge_usage(existing: &mut Option<AgentTokenUsage>, update: Option<AgentTokenUsage>) {
    let Some(update) = update else {
        return;
    };
    match existing {
        Some(existing) => {
            existing.input = add_optional_u64(existing.input, update.input);
            existing.output = add_optional_u64(existing.output, update.output);
        }
        None => *existing = Some(update),
    }
}

fn apply_deferred_queue(prompt: String, queued_prompts: &mut Vec<String>) -> String {
    if queued_prompts.is_empty() {
        return prompt;
    }
    let queued = std::mem::take(queued_prompts).join("\n");
    eprintln!("[note] Prepending queued input from the previous run as context.");
    format!(
        "[Queued follow-up context from the previous run]\n{queued}\n\nCurrent request:\n{prompt}"
    )
}

/// Resolves the provider invocation used by `agents build`.
///
/// Returns an error when route resolution fails, the requested provider/model combination is not
/// valid, or the configured command cannot be constructed for the current prompt.
pub(crate) fn resolve_build_invocation(
    config: &AppConfig,
    planning_meta: &PlanningMeta,
    args: &BuildArgs,
    prompt: &str,
) -> Result<crate::agents::resolution::ResolvedAgentInvocation> {
    let run_args = RunAgentArgs {
        root: Some(args.root.clone()),
        route_key: Some(AGENT_ROUTE_AGENTS_BUILD.to_string()),
        agent: args.agent.clone(),
        prompt: prompt.to_string(),
        instructions: None,
        model: args.model.clone(),
        reasoning: args.reasoning.clone(),
        transport: None,
        attachments: Vec::new(),
    };
    resolve_agent_invocation_for_planning(config, planning_meta, &run_args)
}

fn spawn_output_reader(
    mut reader: impl Read + Send + 'static,
    stream: OutputStream,
    sender: mpsc::Sender<OutputChunk>,
) {
    thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    let text = String::from_utf8_lossy(&buffer[..count]).to_string();
                    if sender.send(OutputChunk { stream, text }).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

fn execute_build_agent(
    session: &mut BuildSession,
    args: &BuildArgs,
    initial_prompt: &str,
    input_rx: &mpsc::Receiver<String>,
) -> Result<BuildRunSummary> {
    let mut prompt = initial_prompt.to_string();
    let mut summary = BuildRunSummary::default();
    let mut continuation: Option<AgentContinuation> = None;

    loop {
        let outcome = execute_build_turn(session, args, &prompt, input_rx, continuation.as_ref())?;
        continuation = outcome.continuation.clone();
        merge_usage(&mut summary.usage, outcome.usage);

        if outcome.queued_prompts.is_empty() {
            break;
        }

        let invocation =
            resolve_build_invocation(&session.config, &session.planning_meta, args, &prompt)?;
        if invocation.builtin_provider && invocation.agent == "codex" {
            if let Some(active_continuation) = continuation.as_ref() {
                summary.resumed_turns += 1;
                let queued = outcome.queued_prompts.join("\n");
                eprintln!(
                    "[note] Resuming Codex session `{}` with queued input.",
                    active_continuation.session_id
                );
                prompt = format!(
                    "[Queued follow-up while the previous turn was still running]\n{queued}"
                );
                continue;
            }
            eprintln!(
                "[note] Codex did not expose a continuation handle; queued input will be applied on the next run."
            );
        } else {
            eprintln!(
                "[note] {} does not support live continuation; queued input will be applied on the next run.",
                provider_display(&invocation.agent, invocation.model.as_deref())
            );
        }

        session.queued_prompts.extend(outcome.queued_prompts);
        break;
    }

    Ok(summary)
}

fn execute_build_turn(
    session: &BuildSession,
    args: &BuildArgs,
    prompt: &str,
    input_rx: &mpsc::Receiver<String>,
    continuation: Option<&AgentContinuation>,
) -> Result<BuildRunOutcome> {
    let invocation =
        resolve_build_invocation(&session.config, &session.planning_meta, args, prompt)?;

    if std::env::var_os("METASTACK_DEBUG").is_some() {
        for line in render_invocation_diagnostics(&invocation) {
            eprintln!("[debug] {line}");
        }
    }

    let continuation_session_id = continuation
        .filter(|state| state.provider == invocation.agent)
        .map(|state| state.session_id.clone());
    let capture_output = invocation.builtin_provider && invocation.agent == "codex";
    let command_args = command_args_for_invocation_with_options(
        &invocation,
        AgentExecutionOptions {
            working_dir: Some(session.workspace_dir.clone()),
            extra_env: Vec::new(),
            capture_output,
            continuation: continuation_session_id,
        },
    )?;
    let attempted = validate_invocation_command_surface(&invocation, &command_args)?;

    let mut command = Command::new(&invocation.command);
    command.args(&command_args);
    command.current_dir(&session.workspace_dir);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    apply_noninteractive_agent_environment(&mut command);
    apply_invocation_environment(&mut command, &invocation, prompt, None);

    let mut child: Child = command.spawn().with_context(|| {
        format!(
            "failed to launch agent `{}` with command `{attempted}`",
            invocation.agent
        )
    })?;

    if invocation.transport == PromptTransport::Stdin {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("failed to open stdin for agent `{}`", invocation.agent))?;
        stdin
            .write_all(invocation.payload.as_bytes())
            .with_context(|| {
                format!(
                    "failed to write prompt payload to agent `{}`",
                    invocation.agent
                )
            })?;
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("failed to open stdout for agent `{}`", invocation.agent))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("failed to open stderr for agent `{}`", invocation.agent))?;
    let (sender, receiver) = mpsc::channel();
    spawn_output_reader(stdout, OutputStream::Stdout, sender.clone());
    spawn_output_reader(stderr, OutputStream::Stderr, sender);

    let mut raw_stdout = String::new();
    let mut raw_stderr = String::new();
    let mut queued_prompts = Vec::new();
    let mut codex_resume_noted = false;

    loop {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => match chunk.stream {
                OutputStream::Stdout => {
                    raw_stdout.push_str(&chunk.text);
                    print!("{}", chunk.text);
                    io::stdout().flush().ok();
                }
                OutputStream::Stderr => {
                    raw_stderr.push_str(&chunk.text);
                    eprint!("{}", chunk.text);
                    io::stderr().flush().ok();
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if INTERRUPTED.load(Ordering::SeqCst) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(anyhow!("agent interrupted by user"));
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        while let Ok(message) = input_rx.try_recv() {
            if message.is_empty() {
                continue;
            }
            eprintln!("[queued] {message}");
            if capture_output && !codex_resume_noted {
                eprintln!(
                    "[note] queued input will resume the active Codex session after this turn completes."
                );
                codex_resume_noted = true;
            }
            queued_prompts.push(message);
        }
    }

    while let Ok(message) = input_rx.try_recv() {
        if message.is_empty() {
            continue;
        }
        eprintln!("[queued] {message}");
        queued_prompts.push(message);
    }

    if INTERRUPTED.load(Ordering::SeqCst) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(anyhow!("agent interrupted by user"));
    }

    let status = child
        .wait()
        .with_context(|| format!("failed to wait for agent `{}`", invocation.agent))?;

    let mut outcome = BuildRunOutcome {
        usage: None,
        continuation: None,
        queued_prompts,
    };
    if capture_output {
        if let Some(provider) = builtin_provider_adapter(&invocation.agent) {
            if let Ok(parsed) = provider.parse_capture_output(&raw_stdout) {
                outcome.usage = parsed.usage;
                outcome.continuation = parsed.continuation.map(|session_id| AgentContinuation {
                    provider: invocation.agent.clone(),
                    session_id,
                });
            }
        }
    }

    if !status.success() {
        let code = status
            .code()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "terminated by signal".to_string());
        bail!(
            "agent `{}` exited unsuccessfully ({code}) while running `{attempted}`: {}",
            invocation.agent,
            raw_stderr.trim()
        );
    }

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_deferred_queue, path_like_workspace_arg, resolve_interactive_workspace_and_prompt,
        resolve_noninteractive_workspace_and_prompt,
    };
    use crate::cli::BuildArgs;
    use std::path::{Path, PathBuf};

    #[test]
    fn build_args_use_dir_as_workspace_and_positional_as_prompt() {
        let args = BuildArgs {
            positionals: vec!["fix auth".to_string()],
            root: PathBuf::from("."),
            agent: None,
            model: None,
            reasoning: None,
            dir: Some(PathBuf::from("/tmp/workspace")),
            max_turns: 20,
            no_interactive: false,
        };

        let source_root = PathBuf::from("/tmp/repo");
        let (workspace, prompt) = resolve_interactive_workspace_and_prompt(&args, &source_root)
            .expect("resolution should succeed");
        assert_eq!(workspace, Some(PathBuf::from("/tmp/workspace")));
        assert_eq!(prompt.as_deref(), Some("fix auth"));
    }

    #[test]
    fn queued_prompts_are_prepended_once() {
        let mut queued = vec!["first".to_string(), "second".to_string()];
        let prompt = apply_deferred_queue("current".to_string(), &mut queued);
        assert!(prompt.contains("first"));
        assert!(prompt.contains("current"));
        assert!(queued.is_empty());
    }

    #[test]
    fn workspace_resolution_accepts_path_like_positionals() {
        let args = BuildArgs {
            positionals: vec!["workspace/MET-45".to_string(), "run qa".to_string()],
            root: PathBuf::from("."),
            agent: None,
            model: None,
            reasoning: None,
            dir: None,
            max_turns: 20,
            no_interactive: false,
        };

        let source_root = PathBuf::from("/tmp/repo");
        let (workspace, prompt) = resolve_interactive_workspace_and_prompt(&args, &source_root)
            .expect("resolution should succeed");
        assert_eq!(workspace, Some(PathBuf::from("workspace/MET-45")));
        assert_eq!(prompt.as_deref(), Some("run qa"));
    }

    #[test]
    fn path_like_workspace_detection_accepts_relative_paths() {
        assert!(path_like_workspace_arg(Path::new("./MET-45")));
        assert!(path_like_workspace_arg(Path::new("workspace/MET-45")));
        assert!(!path_like_workspace_arg(Path::new("MET-45")));
    }

    #[test]
    fn interactive_build_args_allow_workspace_picker_without_explicit_workspace() {
        let args = BuildArgs {
            positionals: Vec::new(),
            root: PathBuf::from("."),
            agent: None,
            model: None,
            reasoning: None,
            dir: None,
            max_turns: 20,
            no_interactive: false,
        };

        let source_root = PathBuf::from("/tmp/repo");
        let (workspace, prompt) = resolve_interactive_workspace_and_prompt(&args, &source_root)
            .expect("interactive picker should resolve");
        assert!(workspace.is_none());
        assert!(prompt.is_none());
    }

    #[test]
    fn noninteractive_build_args_require_workspace() {
        let args = BuildArgs {
            positionals: Vec::new(),
            root: PathBuf::from("."),
            agent: None,
            model: None,
            reasoning: None,
            dir: None,
            max_turns: 20,
            no_interactive: true,
        };

        let source_root = PathBuf::from("/tmp/repo");
        let error = resolve_noninteractive_workspace_and_prompt(&args, &source_root)
            .expect_err("workspace should be required");
        assert!(
            error
                .to_string()
                .contains("workspace identifier is required"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn build_args_reject_extra_dir_positionals() {
        let args = BuildArgs {
            positionals: vec!["prompt".to_string(), "extra".to_string()],
            root: PathBuf::from("."),
            agent: None,
            model: None,
            reasoning: None,
            dir: Some(PathBuf::from("/tmp/workspace")),
            max_turns: 20,
            no_interactive: false,
        };

        let source_root = PathBuf::from("/tmp/repo");
        let error = resolve_interactive_workspace_and_prompt(&args, &source_root)
            .expect_err("extra positionals should fail");
        assert!(
            error
                .to_string()
                .contains("accepts at most one positional prompt"),
            "unexpected error: {error:#}"
        );
    }
}

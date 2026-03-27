use std::collections::VecDeque;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use crate::agents::resolution::ResolvedAgentInvocation;
use crate::agents::{
    AgentContinuation, AgentExecutionOptions, AgentTokenUsage, apply_invocation_environment,
    apply_noninteractive_agent_environment, command_args_for_invocation_with_options,
    resolve_agent_invocation_for_planning, validate_invocation_command_surface,
};
use crate::cli::{BuildArgs, RunAgentArgs};
use crate::config::{
    AGENT_ROUTE_AGENTS_BUILD, AppConfig, PlanningMeta, load_required_planning_meta,
};
use crate::fs::{canonicalize_existing_dir, sibling_workspace_root};
use crate::listen::store::resolve_source_project_root;

#[derive(Debug, Clone)]
struct BuildSession {
    root: PathBuf,
    workspace_path: PathBuf,
    agent: String,
    model: Option<String>,
    reasoning: Option<String>,
    max_turns: u32,
    run_count: u32,
    deferred_prompt: Option<String>,
}

#[derive(Debug, Clone)]
struct BuildTurnRequest {
    prompt: String,
    continuation: Option<AgentContinuation>,
}

#[derive(Debug, Clone)]
struct BuildTurnReport {
    continuation: Option<AgentContinuation>,
    usage: Option<AgentTokenUsage>,
    queued_prompts: Vec<String>,
    cancelled: bool,
    failure: Option<String>,
}

#[derive(Debug)]
enum ActiveInputEvent {
    Queue(String),
    Cancel,
}

#[derive(Debug)]
struct BuildIoCapture {
    stdout: Arc<Mutex<Vec<u8>>>,
    stderr: Arc<Mutex<Vec<u8>>>,
}

impl BuildIoCapture {
    fn new() -> Self {
        Self {
            stdout: Arc::new(Mutex::new(Vec::new())),
            stderr: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn stdout_string(&self) -> Result<String> {
        let bytes = self
            .stdout
            .lock()
            .map_err(|_| anyhow!("failed to lock captured build stdout"))?
            .clone();
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }
}

/// Run `meta agents build` against a resolved workspace directory.
///
/// Returns an error when workspace resolution fails, agent config cannot be resolved, the target
/// directory is not a git repository, prompt collection fails, or a launched agent subprocess
/// exits unsuccessfully.
pub async fn run_build(args: &BuildArgs) -> Result<()> {
    if args.max_turns == 0 {
        bail!("`--max-turns` must be at least 1");
    }

    let requested_root = canonicalize_existing_dir(&args.root)?;
    let root = resolve_source_project_root(&requested_root)?;
    let planning_meta = load_required_planning_meta(&root, "agents build")?;
    let workspace_path = resolve_build_workspace_path(&root, args)?;
    validate_git_repository(&workspace_path)?;
    let initial_prompt = resolve_initial_prompt(args);

    let mut session = resolve_build_session(&root, &planning_meta, &workspace_path, args)?;
    let mut next_request = initial_prompt.map(|prompt| BuildTurnRequest {
        prompt,
        continuation: None,
    });

    if args.no_interactive && next_request.is_none() {
        bail!("`meta agents build --no-interactive` requires a positional prompt");
    }
    if !args.no_interactive
        && next_request.is_none()
        && !(io::stdin().is_terminal() && io::stdout().is_terminal())
    {
        bail!(
            "`meta agents build` requires a prompt when stdin/stdout are not interactive terminals"
        );
    }

    loop {
        if session.run_count >= session.max_turns {
            println!(
                "Reached build run limit ({}) for {}.",
                session.max_turns,
                session.workspace_path.display()
            );
            break;
        }

        let request = match next_request.take() {
            Some(request) => request,
            None => match prompt_for_next_instruction(&mut session)? {
                Some(prompt) => BuildTurnRequest {
                    prompt,
                    continuation: None,
                },
                None => break,
            },
        };

        session.run_count += 1;
        print_status_line(&session);
        let report = run_build_turn(&session, request)?;
        print_completion_summary(&session, &report);

        if report.cancelled {
            continue;
        }

        if let Some(request) = next_request_from_queued_prompts(&mut session, &report) {
            next_request = Some(request);
            continue;
        }

        if let Some(message) = report.failure.as_deref() {
            if args.no_interactive {
                bail!("{message}");
            }
            continue;
        }

        if args.no_interactive {
            break;
        }
    }

    Ok(())
}

fn resolve_build_session(
    root: &Path,
    planning_meta: &PlanningMeta,
    workspace_path: &Path,
    args: &BuildArgs,
) -> Result<BuildSession> {
    let config = AppConfig::load().context("failed to load app config for `agents build`")?;
    let invocation = resolve_agent_invocation_for_planning(
        &config,
        planning_meta,
        &RunAgentArgs {
            root: Some(root.to_path_buf()),
            route_key: Some(AGENT_ROUTE_AGENTS_BUILD.to_string()),
            agent: args.agent.clone(),
            prompt: "Resolve build session defaults".to_string(),
            instructions: None,
            model: args.model.clone(),
            reasoning: args.reasoning.clone(),
            transport: None,
            attachments: Vec::new(),
        },
    )?;

    Ok(BuildSession {
        root: root.to_path_buf(),
        workspace_path: workspace_path.to_path_buf(),
        agent: invocation.agent,
        model: invocation.model,
        reasoning: invocation.reasoning,
        max_turns: args.max_turns,
        run_count: 0,
        deferred_prompt: None,
    })
}

fn resolve_build_workspace_path(root: &Path, args: &BuildArgs) -> Result<PathBuf> {
    if let Some(dir) = args.dir.as_deref() {
        return canonicalize_existing_dir(dir);
    }

    let workspace = args
        .workspace
        .as_deref()
        .ok_or_else(|| anyhow!("`meta agents build` requires a workspace selector or `--dir`"))?;

    if looks_like_path(workspace) {
        return canonicalize_existing_dir(Path::new(workspace));
    }

    let workspace_root = sibling_workspace_root(root)?;
    canonicalize_existing_dir(&workspace_root.join(workspace))
}

fn resolve_initial_prompt(args: &BuildArgs) -> Option<String> {
    match (
        args.dir.as_ref(),
        args.workspace.as_ref(),
        args.prompt.as_ref(),
    ) {
        (Some(_), Some(prompt), None) => Some(prompt.clone()),
        _ => args.prompt.clone(),
    }
}

fn looks_like_path(value: &str) -> bool {
    value.starts_with('.')
        || value.starts_with('~')
        || value.starts_with(std::path::MAIN_SEPARATOR)
        || value.contains(std::path::MAIN_SEPARATOR)
}

fn validate_git_repository(path: &Path) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .with_context(|| format!("failed to validate git repository `{}`", path.display()))?;
    if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != "true" {
        bail!("workspace `{}` is not a git repository", path.display());
    }
    Ok(())
}

fn prompt_for_next_instruction(session: &mut BuildSession) -> Result<Option<String>> {
    let mut stdout = io::stdout().lock();
    write!(stdout, "build> ").context("failed to render build prompt")?;
    stdout.flush().context("failed to flush build prompt")?;

    let mut input = String::new();
    io::stdin()
        .lock()
        .read_line(&mut input)
        .context("failed to read build prompt input")?;
    let trimmed = input.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("exit")
        || trimmed.eq_ignore_ascii_case("quit")
    {
        return Ok(None);
    }

    Ok(Some(apply_deferred_prompt(session, trimmed)))
}

fn apply_deferred_prompt(session: &mut BuildSession, prompt: &str) -> String {
    match session.deferred_prompt.take() {
        Some(deferred) => format!(
            "Queued follow-up from the previous build run:\n{deferred}\n\nNext instruction:\n{prompt}"
        ),
        None => prompt.to_string(),
    }
}

fn print_status_line(session: &BuildSession) {
    let model = session.model.as_deref().unwrap_or("unset");
    let reasoning = session.reasoning.as_deref().unwrap_or("unset");
    println!(
        "[build run #{}/{}] workspace={} provider={} model={} reasoning={}",
        session.run_count,
        session.max_turns,
        session.workspace_path.display(),
        session.agent,
        model,
        reasoning
    );
}

fn print_completion_summary(session: &BuildSession, report: &BuildTurnReport) {
    if report.cancelled {
        println!(
            "[build run #{}] interrupted; returned to build prompt.",
            session.run_count
        );
        return;
    }

    if let Some(message) = report.failure.as_deref() {
        println!("[build run #{}] failed: {message}", session.run_count);
        return;
    }

    if let Some(usage) = report.usage.as_ref() {
        println!(
            "[build run #{}] completed successfully. tokens: in={} out={} total={}",
            session.run_count,
            render_optional_token(usage.input),
            render_optional_token(usage.output),
            usage
                .input
                .zip(usage.output)
                .map(|(input, output)| (input + output).to_string())
                .unwrap_or_else(|| "n/a".to_string())
        );
    } else {
        println!("[build run #{}] completed successfully.", session.run_count);
    }
}

fn render_optional_token(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "n/a".to_string())
}

fn run_build_turn(session: &BuildSession, request: BuildTurnRequest) -> Result<BuildTurnReport> {
    let prompt = request.prompt.clone();
    let run_args = RunAgentArgs {
        root: Some(session.root.clone()),
        route_key: Some(AGENT_ROUTE_AGENTS_BUILD.to_string()),
        agent: Some(session.agent.clone()),
        prompt,
        instructions: None,
        model: session.model.clone(),
        reasoning: session.reasoning.clone(),
        transport: None,
        attachments: Vec::new(),
    };

    let config = AppConfig::load().context("failed to load app config for `agents build`")?;
    let planning_meta = PlanningMeta::load(&session.root).with_context(|| {
        format!(
            "failed to load planning metadata from `{}`",
            session.root.display()
        )
    })?;
    let invocation = resolve_agent_invocation_for_planning(&config, &planning_meta, &run_args)?;
    let mut report = run_build_turn_attempt(
        session,
        &run_args,
        &invocation,
        request.continuation.as_ref(),
    )?;
    if should_retry_without_continuation(&invocation.agent, request.continuation.as_ref(), &report)
    {
        println!(
            "Stored `{}` continuation was rejected; retrying this build prompt as a fresh run.",
            invocation.agent
        );
        report = run_build_turn_attempt(session, &run_args, &invocation, None)?;
    }
    Ok(report)
}

fn run_build_turn_attempt(
    session: &BuildSession,
    run_args: &RunAgentArgs,
    invocation: &ResolvedAgentInvocation,
    continuation: Option<&AgentContinuation>,
) -> Result<BuildTurnReport> {
    let options = AgentExecutionOptions {
        working_dir: Some(session.workspace_path.clone()),
        extra_env: Vec::new(),
        capture_output: invocation.builtin_provider,
        continuation: continuation.map(|state| state.session_id.clone()),
    };
    let command_args = command_args_for_invocation_with_options(invocation, options)?;
    let attempted_command = validate_invocation_command_surface(invocation, &command_args)?;

    let mut command = Command::new(&invocation.command);
    command.current_dir(&session.workspace_path);
    command.args(&command_args);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    apply_noninteractive_agent_environment(&mut command);
    apply_invocation_environment(
        &mut command,
        invocation,
        &run_args.prompt,
        run_args.instructions.as_deref(),
    );

    let mut child = command.spawn().with_context(|| {
        format!(
            "failed to launch agent `{}` with command `{attempted_command}`",
            invocation.agent
        )
    })?;

    if invocation.transport == crate::config::PromptTransport::Stdin {
        let mut stdin = child.stdin.take().ok_or_else(|| {
            anyhow!(
                "failed to open stdin for build agent `{}`",
                invocation.agent
            )
        })?;
        stdin
            .write_all(invocation.payload.as_bytes())
            .with_context(|| {
                format!(
                    "failed to write prompt payload to build agent `{}`",
                    invocation.agent
                )
            })?;
    }

    let capture = BuildIoCapture::new();
    let stdout = child.stdout.take().ok_or_else(|| {
        anyhow!(
            "failed to capture build stdout for agent `{}`",
            invocation.agent
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        anyhow!(
            "failed to capture build stderr for agent `{}`",
            invocation.agent
        )
    })?;
    let stdout_handle = spawn_stream_printer(stdout, io::stdout(), Arc::clone(&capture.stdout));
    let stderr_handle = spawn_stream_printer(stderr, io::stderr(), Arc::clone(&capture.stderr));

    let interactive_input = io::stdin().is_terminal() && io::stdout().is_terminal();
    let stop_input = Arc::new(AtomicBool::new(false));
    let (input_tx, input_rx) = mpsc::channel();
    let input_handle = if interactive_input {
        enable_raw_mode().context("failed to enable raw mode for build input queue")?;
        Some(spawn_active_input_reader(input_tx, Arc::clone(&stop_input)))
    } else {
        None
    };

    let mut queued_prompts = VecDeque::new();
    let mut cancelled = false;
    loop {
        match input_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(ActiveInputEvent::Queue(prompt)) => {
                println!("\n[queued] {prompt}");
                queued_prompts.push_back(prompt);
            }
            Ok(ActiveInputEvent::Cancel) => {
                cancel_child_process(&mut child)?;
                cancelled = true;
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if child
                    .try_wait()
                    .context("failed to poll active build process")?
                    .is_some()
                {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if child
                    .try_wait()
                    .context("failed to poll active build process")?
                    .is_some()
                {
                    break;
                }
            }
        }
    }

    let status = child
        .wait()
        .with_context(|| format!("failed to wait for build agent `{}`", invocation.agent))?;
    stop_input.store(true, Ordering::Relaxed);
    if let Some(handle) = input_handle {
        let _ = handle.join();
        disable_raw_mode().context("failed to disable raw mode after build run")?;
    }

    let _stdout_bytes = stdout_handle
        .join()
        .map_err(|_| anyhow!("build stdout drain thread panicked"))??;
    stderr_handle
        .join()
        .map_err(|_| anyhow!("build stderr drain thread panicked"))??;

    if cancelled {
        return Ok(BuildTurnReport {
            continuation: None,
            usage: None,
            queued_prompts: queued_prompts.into_iter().collect(),
            cancelled: true,
            failure: None,
        });
    }

    if !status.success() {
        let code = status
            .code()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "terminated by signal".to_string());
        let stderr_guard = capture
            .stderr
            .lock()
            .map_err(|_| anyhow!("failed to lock captured build stderr"))?;
        let stderr_text = String::from_utf8_lossy(&stderr_guard);
        return Ok(BuildTurnReport {
            continuation: None,
            usage: None,
            queued_prompts: queued_prompts.into_iter().collect(),
            cancelled: false,
            failure: Some(format!(
                "agent `{}` exited unsuccessfully ({code}) while running `{attempted_command}`: {}",
                invocation.agent,
                stderr_text.trim()
            )),
        });
    }

    let mut continuation = None;
    let mut usage = None;
    if invocation.builtin_provider {
        let provider = crate::agent_provider::builtin_provider_adapter(&invocation.agent)
            .ok_or_else(|| anyhow!("builtin provider `{}` is not configured", invocation.agent))?;
        let parsed = provider.parse_capture_output(&capture.stdout_string()?)?;
        usage = parsed.usage;
        continuation = if invocation.agent == "codex" {
            parsed.continuation.map(|session_id| AgentContinuation {
                provider: invocation.agent.clone(),
                session_id,
            })
        } else {
            None
        };
    }

    Ok(BuildTurnReport {
        continuation,
        usage,
        queued_prompts: queued_prompts.into_iter().collect(),
        cancelled: false,
        failure: None,
    })
}

fn should_retry_without_continuation(
    agent: &str,
    continuation: Option<&AgentContinuation>,
    report: &BuildTurnReport,
) -> bool {
    let Some(message) = report.failure.as_deref() else {
        return false;
    };
    let Some(continuation) = continuation else {
        return false;
    };
    if continuation.provider != agent {
        return false;
    }
    crate::agent_provider::builtin_provider_adapter(agent)
        .is_some_and(|provider| provider.is_invalid_resume_error(message))
}

fn next_request_from_queued_prompts(
    session: &mut BuildSession,
    report: &BuildTurnReport,
) -> Option<BuildTurnRequest> {
    let queued_prompt = combine_queued_prompts(&report.queued_prompts)?;
    if session.agent == "codex" {
        if let Some(continuation) = report.continuation.clone() {
            return Some(BuildTurnRequest {
                prompt: queued_prompt,
                continuation: Some(continuation),
            });
        }
        println!(
            "Queued follow-up could not resume the active Codex session; it will be prepended to the next build prompt."
        );
    } else {
        println!(
            "Queued follow-up will be prepended to the next build prompt because `{}` does not use native build-session continuation.",
            session.agent
        );
    }
    session.deferred_prompt = Some(match session.deferred_prompt.take() {
        Some(existing) => format!("{existing}\n\n{queued_prompt}"),
        None => queued_prompt,
    });
    None
}

fn combine_queued_prompts(queued_prompts: &[String]) -> Option<String> {
    if queued_prompts.is_empty() {
        None
    } else {
        Some(queued_prompts.join("\n\n"))
    }
}

fn spawn_stream_printer(
    mut reader: impl Read + Send + 'static,
    mut writer: impl Write + Send + 'static,
    capture: Arc<Mutex<Vec<u8>>>,
) -> thread::JoinHandle<Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        let mut collected = Vec::new();
        loop {
            let count = reader
                .read(&mut buffer)
                .context("failed to read build process output")?;
            if count == 0 {
                break;
            }
            writer
                .write_all(&buffer[..count])
                .context("failed to stream build process output")?;
            writer
                .flush()
                .context("failed to flush build process output")?;
            collected.extend_from_slice(&buffer[..count]);
            capture
                .lock()
                .map_err(|_| anyhow!("failed to lock build process capture buffer"))?
                .extend_from_slice(&buffer[..count]);
        }
        Ok(collected)
    })
}

fn spawn_active_input_reader(
    sender: mpsc::Sender<ActiveInputEvent>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = String::new();
        while !stop.load(Ordering::Relaxed) {
            match event::poll(Duration::from_millis(100)) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(_) => break,
            }
            let Ok(Event::Key(key)) = event::read() else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            let ctrl_c =
                key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
            if ctrl_c {
                let _ = sender.send(ActiveInputEvent::Cancel);
                buffer.clear();
                continue;
            }
            match key.code {
                KeyCode::Enter => {
                    let queued = buffer.trim().to_string();
                    if !queued.is_empty() {
                        let _ = sender.send(ActiveInputEvent::Queue(queued));
                    }
                    buffer.clear();
                }
                KeyCode::Backspace => {
                    buffer.pop();
                }
                KeyCode::Esc => {
                    buffer.clear();
                }
                KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    buffer.push(ch);
                }
                _ => {}
            }
        }
    })
}

fn cancel_child_process(child: &mut Child) -> Result<()> {
    #[cfg(unix)]
    {
        let pid = child.id().to_string();
        let status = Command::new("kill")
            .arg("-INT")
            .arg(&pid)
            .status()
            .with_context(|| format!("failed to run `kill -INT {pid}`"))?;
        if !status.success() {
            child
                .kill()
                .context("failed to force-stop active build process")?;
        }
    }
    #[cfg(not(unix))]
    {
        child
            .kill()
            .context("failed to stop active build process")?;
    }

    for _ in 0..20 {
        if child
            .try_wait()
            .context("failed to poll interrupted build process")?
            .is_some()
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }

    child
        .kill()
        .context("failed to force-stop interrupted build process")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        BuildSession, BuildTurnReport, combine_queued_prompts, looks_like_path,
        next_request_from_queued_prompts, resolve_build_workspace_path, resolve_initial_prompt,
        should_retry_without_continuation,
    };
    use crate::agents::AgentContinuation;
    use crate::cli::BuildArgs;
    use anyhow::Result;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn build_args() -> BuildArgs {
        BuildArgs {
            workspace: None,
            prompt: None,
            root: PathBuf::from("."),
            agent: None,
            model: None,
            reasoning: None,
            dir: None,
            max_turns: 20,
            no_interactive: false,
        }
    }

    fn build_session(agent: &str) -> BuildSession {
        BuildSession {
            root: PathBuf::from("/repo"),
            workspace_path: PathBuf::from("/repo-workspace/ENG-10507"),
            agent: agent.to_string(),
            model: None,
            reasoning: None,
            max_turns: 20,
            run_count: 0,
            deferred_prompt: None,
        }
    }

    #[test]
    fn resolve_initial_prompt_uses_workspace_as_prompt_for_dir_runs() {
        let mut args = build_args();
        args.dir = Some(PathBuf::from("/tmp/workspace"));
        args.workspace = Some("fix the auth bug".to_string());

        assert_eq!(
            resolve_initial_prompt(&args).as_deref(),
            Some("fix the auth bug")
        );
    }

    #[test]
    fn looks_like_path_requires_path_markers() {
        assert!(looks_like_path("./workspace"));
        assert!(looks_like_path("/tmp/workspace"));
        assert!(looks_like_path("nested/workspace"));
        assert!(!looks_like_path("ENG-10507"));
    }

    #[test]
    fn resolve_build_workspace_path_uses_sibling_workspace_for_ticket_ids() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let workspace_root = temp.path().join("repo-workspace");
        let workspace_path = workspace_root.join("ENG-10507");
        fs::create_dir_all(&repo_root)?;
        fs::create_dir_all(&workspace_path)?;

        let mut args = build_args();
        args.workspace = Some("ENG-10507".to_string());

        assert_eq!(
            resolve_build_workspace_path(&repo_root, &args)?,
            workspace_path.canonicalize()?
        );
        Ok(())
    }

    #[test]
    fn resolve_build_workspace_path_prefers_explicit_dir() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let explicit_dir = temp.path().join("custom-workspace");
        fs::create_dir_all(&repo_root)?;
        fs::create_dir_all(&explicit_dir)?;

        let mut args = build_args();
        args.workspace = Some("ENG-10507".to_string());
        args.dir = Some(explicit_dir.clone());

        assert_eq!(
            resolve_build_workspace_path(&repo_root, &args)?,
            explicit_dir.canonicalize()?
        );
        Ok(())
    }

    #[test]
    fn next_request_from_queued_prompts_uses_codex_continuation_when_available() {
        let mut session = build_session("codex");
        let report = BuildTurnReport {
            continuation: Some(AgentContinuation {
                provider: "codex".to_string(),
                session_id: "thread-123".to_string(),
            }),
            usage: None,
            queued_prompts: vec!["keep going".to_string()],
            cancelled: false,
            failure: None,
        };

        let next = next_request_from_queued_prompts(&mut session, &report)
            .expect("codex continuation should create an immediate follow-up request");

        assert_eq!(next.prompt, "keep going");
        assert_eq!(
            next.continuation,
            Some(AgentContinuation {
                provider: "codex".to_string(),
                session_id: "thread-123".to_string(),
            })
        );
        assert!(session.deferred_prompt.is_none());
    }

    #[test]
    fn next_request_from_queued_prompts_defers_when_continuation_is_unavailable() {
        let mut session = build_session("claude");
        let report = BuildTurnReport {
            continuation: None,
            usage: None,
            queued_prompts: vec!["apply the queued fix".to_string()],
            cancelled: false,
            failure: None,
        };

        assert!(next_request_from_queued_prompts(&mut session, &report).is_none());
        assert_eq!(
            session.deferred_prompt.as_deref(),
            Some("apply the queued fix")
        );
    }

    #[test]
    fn should_retry_without_continuation_only_for_invalid_resume_failures() {
        let report = BuildTurnReport {
            continuation: None,
            usage: None,
            queued_prompts: Vec::new(),
            cancelled: false,
            failure: Some(
                "agent `claude` exited unsuccessfully (1) while running `claude -p`: --resume requires a valid session id"
                    .to_string(),
            ),
        };

        assert!(should_retry_without_continuation(
            "claude",
            Some(&AgentContinuation {
                provider: "claude".to_string(),
                session_id: "session-1".to_string(),
            }),
            &report,
        ));
        assert!(!should_retry_without_continuation(
            "claude",
            Some(&AgentContinuation {
                provider: "codex".to_string(),
                session_id: "session-1".to_string(),
            }),
            &report,
        ));
    }

    #[test]
    fn combine_queued_prompts_joins_multiple_entries() {
        assert_eq!(
            combine_queued_prompts(&["one".to_string(), "two".to_string()]).as_deref(),
            Some("one\n\ntwo")
        );
    }
}

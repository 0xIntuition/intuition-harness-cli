use std::io::{self, BufRead, Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};

use crate::agent_provider::builtin_provider_adapter;
use crate::agents::{
    AgentContinuation, AgentExecutionOptions, apply_invocation_environment,
    apply_noninteractive_agent_environment, command_args_for_invocation_with_options,
    render_invocation_diagnostics, resolve_agent_invocation_for_planning,
    validate_invocation_command_surface,
};
use crate::cli::{BuildArgs, RunAgentArgs};
use crate::config::{
    AGENT_ROUTE_AGENTS_BUILD, AppConfig, PlanningMeta, PromptTransport, resolve_agent_config,
    AgentConfigOverrides, normalize_agent_name,
};
use crate::fs::{canonicalize_existing_dir, sibling_workspace_root};

// ---------------------------------------------------------------------------
// SIGINT handling
// ---------------------------------------------------------------------------

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn sigint_handler(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

/// Installs a custom SIGINT handler that sets the [`INTERRUPTED`] flag instead of terminating.
fn install_sigint_handler() {
    unsafe {
        libc::signal(libc::SIGINT, sigint_handler as *const () as libc::sighandler_t);
    }
}

/// Restores the default SIGINT handler (process termination).
fn restore_default_sigint() {
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_DFL);
    }
}

// ---------------------------------------------------------------------------
// Workspace resolution
// ---------------------------------------------------------------------------

/// Resolves the workspace directory and effective prompt from [`BuildArgs`].
///
/// When `--dir` is provided, uses it as the workspace directory and re-interprets the positional
/// `workspace` argument as the prompt if no explicit prompt was given. When `--dir` is absent,
/// resolves the positional `workspace` as a ticket ID via the sibling workspace root.
fn resolve_workspace_and_prompt(args: &BuildArgs) -> Result<(PathBuf, Option<String>)> {
    if let Some(dir) = &args.dir {
        let prompt = args.prompt.clone().or_else(|| args.workspace.clone());
        Ok((dir.clone(), prompt))
    } else {
        let workspace_id = args
            .workspace
            .as_ref()
            .ok_or_else(|| anyhow!("workspace identifier is required when --dir is not provided"))?;
        let root = canonicalize_existing_dir(&args.root)?;
        let workspace_root = sibling_workspace_root(&root)?;
        let workspace_dir = workspace_root.join(workspace_id);
        Ok((workspace_dir, args.prompt.clone()))
    }
}

/// Validates that the given path exists and is a git repository.
fn validate_git_repo(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!(
            "workspace directory does not exist: `{}`",
            path.display()
        );
    }
    if !path.is_dir() {
        bail!(
            "workspace path is not a directory: `{}`",
            path.display()
        );
    }
    let git_dir = path.join(".git");
    if !git_dir.exists() {
        bail!(
            "workspace directory is not a git repository: `{}`",
            path.display()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Provider display name
// ---------------------------------------------------------------------------

/// Resolves a human-readable provider label for the status line.
fn resolve_provider_display(
    config: &AppConfig,
    planning_meta: &PlanningMeta,
    args: &BuildArgs,
) -> String {
    let resolved = resolve_agent_config(
        config,
        planning_meta,
        Some(AGENT_ROUTE_AGENTS_BUILD),
        AgentConfigOverrides {
            provider: args.agent.clone(),
            model: args.model.clone(),
            reasoning: args.reasoning.clone(),
        },
    );
    match resolved {
        Ok(r) => {
            let name = normalize_agent_name(&r.provider);
            match r.model {
                Some(model) => format!("{name} ({model})"),
                None => name,
            }
        }
        Err(_) => "unknown".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Output streaming helpers
// ---------------------------------------------------------------------------

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

fn spawn_output_reader(
    mut reader: impl Read + Send + 'static,
    stream: OutputStream,
    sender: mpsc::Sender<OutputChunk>,
) {
    thread::spawn(move || {
        let mut buffer = [0u8; 1024];
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

// ---------------------------------------------------------------------------
// Build session
// ---------------------------------------------------------------------------

/// In-memory session state for the interactive build loop.
struct BuildSession {
    workspace_dir: PathBuf,
    config: AppConfig,
    planning_meta: PlanningMeta,
    run_count: u32,
    continuation: Option<AgentContinuation>,
    queued_prompts: Vec<String>,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Runs the `agents build` command.
///
/// Resolves the workspace directory from the provided ticket ID or explicit `--dir` path, validates
/// it is a git repository, resolves the provider/model/reasoning configuration via the standard
/// precedence chain with the `agents.build` route key, and enters an interactive prompt loop that
/// spawns headless agent runs.
pub async fn run_build(args: &BuildArgs) -> Result<()> {
    let (workspace_dir, effective_prompt) = resolve_workspace_and_prompt(args)?;
    validate_git_repo(&workspace_dir)?;

    let config = AppConfig::load()?;
    let planning_meta = PlanningMeta::load(&args.root).unwrap_or_default();
    let provider_display = resolve_provider_display(&config, &planning_meta, args);

    // Spawn a dedicated stdin reader thread so that user input can be collected both at the
    // interactive prompt and while an agent is running (queued input).
    let (input_tx, input_rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let stdin = io::stdin();
        loop {
            let mut line = String::new();
            match stdin.lock().read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim_end_matches('\n').trim_end_matches('\r').to_string();
                    if input_tx.send(trimmed).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut session = BuildSession {
        workspace_dir,
        config,
        planning_meta,
        run_count: 0,
        continuation: None,
        queued_prompts: Vec::new(),
    };

    let mut initial_prompt = effective_prompt;

    loop {
        // --- Obtain the next prompt ---
        let prompt = if let Some(p) = initial_prompt.take() {
            p
        } else {
            if args.no_interactive {
                break;
            }
            // At the prompt, restore default SIGINT so Ctrl+C exits.
            restore_default_sigint();
            eprint!("build> ");
            io::stderr().flush().ok();
            match input_rx.recv() {
                Ok(line) => {
                    if line.is_empty() || line == "exit" || line == "quit" {
                        break;
                    }
                    line
                }
                Err(_) => break,
            }
        };

        // Check max-turns limit.
        if session.run_count >= args.max_turns {
            eprintln!(
                "Reached maximum number of runs ({}). Exiting.",
                args.max_turns
            );
            break;
        }

        session.run_count += 1;
        INTERRUPTED.store(false, Ordering::SeqCst);

        // Install custom SIGINT handler so Ctrl+C during agent execution does not exit the loop.
        install_sigint_handler();

        // Prepend queued prompts from the previous run if any.
        let full_prompt = if session.queued_prompts.is_empty() {
            prompt
        } else {
            let queued = session.queued_prompts.drain(..).collect::<Vec<_>>().join("\n");
            eprintln!("[note] Prepending queued input from previous run as context");
            format!(
                "[Context from queued input during previous run]\n{queued}\n\n{prompt}"
            )
        };

        // Status line.
        eprintln!(
            "─── Run #{} | {} | {} ───",
            session.run_count,
            session.workspace_dir.display(),
            &provider_display,
        );

        // Execute the agent.
        let result = execute_build_agent(&mut session, args, &full_prompt, &input_rx);

        match &result {
            Ok(()) => eprintln!("─── Run #{} complete ───", session.run_count),
            Err(_) if INTERRUPTED.load(Ordering::SeqCst) => {
                eprintln!("─── Run #{} interrupted ───", session.run_count);
            }
            Err(e) => eprintln!("─── Run #{} failed: {e} ───", session.run_count),
        }

        if args.no_interactive {
            if let Err(e) = result {
                if !INTERRUPTED.load(Ordering::SeqCst) {
                    return Err(e);
                }
            }
            break;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Agent execution
// ---------------------------------------------------------------------------

/// Executes a single agent run with streaming output.
///
/// Resolves the full agent invocation, spawns the subprocess, streams stdout/stderr to the
/// terminal, and collects any queued user input received during execution.
fn execute_build_agent(
    session: &mut BuildSession,
    args: &BuildArgs,
    prompt: &str,
    input_rx: &mpsc::Receiver<String>,
) -> Result<()> {
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

    let invocation =
        resolve_agent_invocation_for_planning(&session.config, &session.planning_meta, &run_args)?;

    // Log resolved invocation diagnostics to stderr when METASTACK_DEBUG is set.
    if std::env::var_os("METASTACK_DEBUG").is_some() {
        for line in render_invocation_diagnostics(&invocation) {
            eprintln!("[debug] {line}");
        }
    }

    let continuation_session_id = session
        .continuation
        .as_ref()
        .filter(|c| c.provider == invocation.agent)
        .map(|c| c.session_id.clone());

    let command_args = command_args_for_invocation_with_options(
        &invocation,
        AgentExecutionOptions {
            working_dir: Some(session.workspace_dir.clone()),
            extra_env: Vec::new(),
            capture_output: invocation.builtin_provider,
            continuation: continuation_session_id,
        },
    )?;

    let attempted = validate_invocation_command_surface(&invocation, &command_args)?;

    let mut command = Command::new(&invocation.command);
    command.args(&command_args);
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

    // Write prompt payload to stdin for Stdin transport providers.
    if invocation.transport == PromptTransport::Stdin {
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(invocation.payload.as_bytes())
                .with_context(|| {
                    format!(
                        "failed to write prompt payload to agent `{}`",
                        invocation.agent
                    )
                })?;
        }
    }
    // Drop stdin handle so the agent can proceed.
    drop(child.stdin.take());

    // Spawn output reader threads.
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

    // Stream output to terminal and handle queued input / interruption.
    let mut raw_stdout = String::new();
    loop {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                match chunk.stream {
                    OutputStream::Stdout => {
                        raw_stdout.push_str(&chunk.text);
                        print!("{}", chunk.text);
                        io::stdout().flush().ok();
                    }
                    OutputStream::Stderr => {
                        eprint!("{}", chunk.text);
                        io::stderr().flush().ok();
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Check for interruption.
                if INTERRUPTED.load(Ordering::SeqCst) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(anyhow!("agent interrupted by user"));
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // Drain any queued user input received during agent execution.
        while let Ok(msg) = input_rx.try_recv() {
            if !msg.is_empty() {
                eprintln!("[queued] {msg}");
                session.queued_prompts.push(msg);
            }
        }
    }

    // Final drain of queued input after output streams close.
    while let Ok(msg) = input_rx.try_recv() {
        if !msg.is_empty() {
            eprintln!("[queued] {msg}");
            session.queued_prompts.push(msg);
        }
    }

    // Check for late interruption.
    if INTERRUPTED.load(Ordering::SeqCst) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(anyhow!("agent interrupted by user"));
    }

    let status = child
        .wait()
        .with_context(|| format!("failed to wait for agent `{}`", invocation.agent))?;

    // Attempt to parse continuation handle from captured output (builtin providers only).
    if invocation.builtin_provider {
        if let Some(provider) = builtin_provider_adapter(&invocation.agent) {
            if let Ok(parsed) = provider.parse_capture_output(&raw_stdout) {
                session.continuation =
                    parsed
                        .continuation
                        .map(|session_id| AgentContinuation {
                            provider: invocation.agent.clone(),
                            session_id,
                        });
            }
        }
    }

    if !status.success() {
        let code = status
            .code()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "terminated by signal".to_string());
        bail!(
            "agent `{}` exited unsuccessfully ({code})",
            invocation.agent,
        );
    }

    Ok(())
}

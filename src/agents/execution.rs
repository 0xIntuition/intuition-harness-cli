use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};

use crate::cli::RunAgentArgs;
use crate::config::{
    AgentConfigOverrides, AppConfig, PlanningMeta, PromptTransport, normalize_agent_name,
    resolve_agent_config,
};
use crate::fs::canonicalize_existing_dir;

#[derive(Debug, Clone, Default)]
pub(crate) struct AgentExecutionOptions {
    pub(crate) working_dir: Option<PathBuf>,
    pub(crate) extra_env: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct AgentCaptureReport {
    pub stdout: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedAgentInvocation {
    pub(crate) agent: String,
    pub(crate) command: String,
    pub(crate) args: Vec<String>,
    pub(crate) model: Option<String>,
    pub(crate) reasoning: Option<String>,
    pub(crate) transport: PromptTransport,
    pub(crate) payload: String,
    pub(crate) builtin_preset: bool,
}

pub fn run_agent_capture(args: &RunAgentArgs) -> Result<AgentCaptureReport> {
    let config = AppConfig::load()?;
    let planning_meta = match args.root.as_deref() {
        Some(root) => PlanningMeta::load(root)?,
        None => PlanningMeta::default(),
    };
    let invocation = resolve_agent_invocation_for_planning(&config, &planning_meta, args)?;
    let command_args = command_args_for_invocation(&invocation, None)?;

    let mut command = Command::new(&invocation.command);
    command.args(&command_args);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.env("METASTACK_AGENT_NAME", &invocation.agent);
    command.env("METASTACK_AGENT_PROMPT", &args.prompt);
    command.env(
        "METASTACK_AGENT_INSTRUCTIONS",
        args.instructions.as_deref().unwrap_or(""),
    );
    command.env(
        "METASTACK_AGENT_MODEL",
        invocation.model.as_deref().unwrap_or(""),
    );
    command.env(
        "METASTACK_AGENT_REASONING",
        invocation.reasoning.as_deref().unwrap_or(""),
    );

    let mut child = command.spawn().with_context(|| {
        format!(
            "failed to launch agent `{}` with command `{}`",
            invocation.agent, invocation.command
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

    let output = child
        .wait_with_output()
        .with_context(|| format!("failed to wait for agent `{}`", invocation.agent))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let code = output
            .status
            .code()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "terminated by signal".to_string());
        bail!(
            "agent `{}` exited unsuccessfully ({code}): {}",
            invocation.agent,
            stderr.trim()
        );
    }

    Ok(AgentCaptureReport { stdout })
}

pub(crate) fn resolve_agent_invocation_for_planning(
    config: &AppConfig,
    planning_meta: &PlanningMeta,
    args: &RunAgentArgs,
) -> Result<ResolvedAgentInvocation> {
    let resolved = resolve_agent_config(
        config,
        planning_meta,
        AgentConfigOverrides {
            provider: args.agent.clone(),
            model: args.model.clone(),
            reasoning: args.reasoning.clone(),
        },
    )?;
    let agent_name = normalize_agent_name(&resolved.provider);
    let builtin_preset = !config.agents.commands.contains_key(&agent_name)
        && crate::config::builtin_agent_definition(&agent_name).is_some();
    let mut definition = config
        .resolve_agent_definition(&agent_name)
        .ok_or_else(|| anyhow!("agent `{agent_name}` is not configured"))?;

    if let Some(transport) = args.transport {
        definition.transport = transport.into();
    }

    let model = resolved.model;
    let reasoning = resolved.reasoning;
    let payload = render_agent_payload(
        &args.prompt,
        args.instructions.as_deref(),
        model.as_deref(),
        reasoning.as_deref(),
    );
    let mut rendered_args = render_command_args(
        &definition.args,
        &args.prompt,
        args.instructions.as_deref(),
        model.as_deref(),
        reasoning.as_deref(),
        &payload,
    );
    if definition.transport == PromptTransport::Arg
        && !definition
            .args
            .iter()
            .any(|arg| arg.contains("{{payload}}") || arg.contains("{{prompt}}"))
    {
        rendered_args.push(payload.clone());
    }

    Ok(ResolvedAgentInvocation {
        agent: agent_name,
        command: definition.command,
        args: rendered_args,
        model,
        reasoning,
        transport: definition.transport,
        payload,
        builtin_preset,
    })
}

pub(crate) fn command_args_for_invocation(
    invocation: &ResolvedAgentInvocation,
    working_dir: Option<&Path>,
) -> Result<Vec<String>> {
    command_args_for_options(
        invocation,
        AgentExecutionOptions {
            working_dir: working_dir.map(Path::to_path_buf),
            extra_env: Vec::new(),
        },
    )
}

fn command_args_for_options(
    invocation: &ResolvedAgentInvocation,
    options: AgentExecutionOptions,
) -> Result<Vec<String>> {
    if !invocation.builtin_preset || invocation.agent != "codex" {
        return Ok(invocation.args.clone());
    }

    let mut args = vec![
        "--sandbox".to_string(),
        "workspace-write".to_string(),
        "--ask-for-approval".to_string(),
        "never".to_string(),
    ];

    if let Some(working_dir) = options.working_dir.as_deref() {
        let workspace = canonicalize_existing_dir(working_dir)?;
        args.push("--cd".to_string());
        args.push(workspace.display().to_string());

        for writable_root in codex_additional_writable_roots(&workspace)? {
            args.push("--add-dir".to_string());
            args.push(writable_root.display().to_string());
        }
    }

    args.extend(invocation.args.clone());
    Ok(args)
}

fn codex_additional_writable_roots(workspace: &Path) -> Result<Vec<PathBuf>> {
    let mut writable_roots = Vec::new();

    for args in [
        ["rev-parse", "--path-format=absolute", "--git-dir"].as_slice(),
        ["rev-parse", "--path-format=absolute", "--git-common-dir"].as_slice(),
    ] {
        let path = git_stdout(workspace, args)?;
        let candidate = PathBuf::from(path);
        if candidate.exists() && candidate != workspace {
            writable_roots.push(candidate);
        }
    }

    writable_roots.sort();
    writable_roots.dedup();
    Ok(writable_roots)
}

pub(crate) fn git_stdout(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .with_context(|| format!("failed to run `git {}`", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn render_agent_payload(
    prompt: &str,
    instructions: Option<&str>,
    model: Option<&str>,
    reasoning: Option<&str>,
) -> String {
    let instructions = instructions
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let model = model.map(str::trim).filter(|value| !value.is_empty());
    let reasoning = reasoning.map(str::trim).filter(|value| !value.is_empty());

    if instructions.is_none() && model.is_none() && reasoning.is_none() {
        return prompt.to_string();
    }

    let mut sections = vec![format!("Prompt:\n{prompt}")];

    if let Some(instructions) = instructions {
        sections.push(format!("Additional instructions:\n{instructions}"));
    }

    if let Some(model) = model {
        sections.push(format!("Preferred model:\n{model}"));
    }

    if let Some(reasoning) = reasoning {
        sections.push(format!("Preferred reasoning effort:\n{reasoning}"));
    }

    sections.join("\n\n")
}

fn render_command_args(
    template: &[String],
    prompt: &str,
    instructions: Option<&str>,
    model: Option<&str>,
    reasoning: Option<&str>,
    payload: &str,
) -> Vec<String> {
    let model_arg = model
        .map(|value| format!("--model={value}"))
        .unwrap_or_default();
    let reasoning_arg = reasoning
        .map(|value| format!("--reasoning={value}"))
        .unwrap_or_default();

    template
        .iter()
        .filter_map(|value| {
            let rendered = value
                .replace("{{prompt}}", prompt)
                .replace("{{instructions}}", instructions.unwrap_or(""))
                .replace("{{model}}", model.unwrap_or(""))
                .replace("{{reasoning}}", reasoning.unwrap_or(""))
                .replace("{{model_arg}}", &model_arg)
                .replace("{{reasoning_arg}}", &reasoning_arg)
                .replace("{{payload}}", payload);

            if rendered.is_empty() {
                None
            } else {
                Some(rendered)
            }
        })
        .collect()
}

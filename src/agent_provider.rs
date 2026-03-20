use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

use crate::config::{AgentCommandConfig, PromptTransport, normalize_agent_name};
use crate::fs::canonicalize_existing_dir;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinInvocationContext {
    Planning,
    Scan,
    Listen,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinReasoningOption {
    pub key: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinModelCatalogEntry {
    pub key: &'static str,
    pub reasoning_options: &'static [BuiltinReasoningOption],
}

pub trait BuiltinProviderAdapter: Sync {
    fn models(&self) -> &'static [BuiltinModelCatalogEntry];
    fn launch_command(&self) -> &'static str;
    fn launch_args(&self, model: Option<&str>, reasoning: Option<&str>) -> Vec<String>;
    fn transport(&self) -> PromptTransport;
    fn prepare_command_args(
        &self,
        launch_args: &[String],
        working_dir: Option<&Path>,
        context: BuiltinInvocationContext,
        transport: PromptTransport,
        capture_output: bool,
        continuation: Option<&str>,
    ) -> Result<Vec<String>>;
    fn validate_command_args(&self, command_args: &[String]) -> Result<()>;
    fn parse_capture_output(&self, raw_stdout: &str) -> Result<(String, Option<String>)>;
    fn is_invalid_resume_error(&self, message: &str) -> bool;

    fn command_definition(&self) -> AgentCommandConfig {
        AgentCommandConfig {
            command: self.launch_command().to_string(),
            args: Vec::new(),
            transport: self.transport(),
        }
    }
}

const REASONING_LOW: BuiltinReasoningOption = BuiltinReasoningOption {
    key: "low",
    label: "Low",
};
const REASONING_MEDIUM: BuiltinReasoningOption = BuiltinReasoningOption {
    key: "medium",
    label: "Medium",
};
const REASONING_HIGH: BuiltinReasoningOption = BuiltinReasoningOption {
    key: "high",
    label: "High",
};
const REASONING_MAX: BuiltinReasoningOption = BuiltinReasoningOption {
    key: "max",
    label: "Max",
};

const REASONING_LOW_MEDIUM_HIGH: &[BuiltinReasoningOption] =
    &[REASONING_LOW, REASONING_MEDIUM, REASONING_HIGH];
const REASONING_LOW_MEDIUM_HIGH_MAX: &[BuiltinReasoningOption] = &[
    REASONING_LOW,
    REASONING_MEDIUM,
    REASONING_HIGH,
    REASONING_MAX,
];

const CODEX_MODELS: &[BuiltinModelCatalogEntry] = &[
    BuiltinModelCatalogEntry {
        key: "gpt-5.4",
        reasoning_options: REASONING_LOW_MEDIUM_HIGH,
    },
    BuiltinModelCatalogEntry {
        key: "gpt-5.3-codex",
        reasoning_options: REASONING_LOW_MEDIUM_HIGH,
    },
    BuiltinModelCatalogEntry {
        key: "gpt-5.2-codex",
        reasoning_options: REASONING_LOW_MEDIUM_HIGH,
    },
    BuiltinModelCatalogEntry {
        key: "gpt-5.1-codex-max",
        reasoning_options: REASONING_LOW_MEDIUM_HIGH,
    },
    BuiltinModelCatalogEntry {
        key: "gpt-5.1-codex",
        reasoning_options: REASONING_LOW_MEDIUM_HIGH,
    },
    BuiltinModelCatalogEntry {
        key: "gpt-5.1-codex-mini",
        reasoning_options: REASONING_LOW_MEDIUM_HIGH,
    },
    BuiltinModelCatalogEntry {
        key: "gpt-5-codex",
        reasoning_options: REASONING_LOW_MEDIUM_HIGH,
    },
    BuiltinModelCatalogEntry {
        key: "gpt-5-codex-mini",
        reasoning_options: REASONING_LOW_MEDIUM_HIGH,
    },
];

const CLAUDE_MODELS: &[BuiltinModelCatalogEntry] = &[
    BuiltinModelCatalogEntry {
        key: "sonnet",
        reasoning_options: REASONING_LOW_MEDIUM_HIGH_MAX,
    },
    BuiltinModelCatalogEntry {
        key: "opus",
        reasoning_options: REASONING_LOW_MEDIUM_HIGH_MAX,
    },
    BuiltinModelCatalogEntry {
        key: "haiku",
        reasoning_options: REASONING_LOW_MEDIUM_HIGH_MAX,
    },
    BuiltinModelCatalogEntry {
        key: "sonnet[1m]",
        reasoning_options: REASONING_LOW_MEDIUM_HIGH_MAX,
    },
    BuiltinModelCatalogEntry {
        key: "opusplan",
        reasoning_options: REASONING_LOW_MEDIUM_HIGH_MAX,
    },
];

struct CodexProviderAdapter;
struct ClaudeProviderAdapter;

impl BuiltinProviderAdapter for CodexProviderAdapter {
    fn models(&self) -> &'static [BuiltinModelCatalogEntry] {
        CODEX_MODELS
    }

    fn launch_command(&self) -> &'static str {
        "codex"
    }

    fn launch_args(&self, model: Option<&str>, reasoning: Option<&str>) -> Vec<String> {
        let mut args = vec!["exec".to_string()];
        if let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) {
            args.push(format!("--model={model}"));
        }
        if let Some(reasoning) = reasoning.map(str::trim).filter(|value| !value.is_empty()) {
            args.push("-c".to_string());
            args.push(format!("reasoning.effort=\"{reasoning}\""));
        }
        args
    }

    fn transport(&self) -> PromptTransport {
        PromptTransport::Arg
    }

    fn prepare_command_args(
        &self,
        launch_args: &[String],
        working_dir: Option<&Path>,
        context: BuiltinInvocationContext,
        transport: PromptTransport,
        capture_output: bool,
        continuation: Option<&str>,
    ) -> Result<Vec<String>> {
        let mut args = match context {
            BuiltinInvocationContext::Listen => {
                vec!["--dangerously-bypass-approvals-and-sandbox".to_string()]
            }
            _ => vec![
                "--sandbox".to_string(),
                "workspace-write".to_string(),
                "--ask-for-approval".to_string(),
                "never".to_string(),
            ],
        };

        if let Some(working_dir) = working_dir {
            let workspace = canonicalize_existing_dir(working_dir)?;
            args.push("--cd".to_string());
            args.push(workspace.display().to_string());

            for writable_root in codex_additional_writable_roots(&workspace)? {
                args.push("--add-dir".to_string());
                args.push(writable_root.display().to_string());
            }
        }

        let prompt_arg = if transport == PromptTransport::Arg {
            launch_args.last().cloned()
        } else {
            None
        };
        let exec_args_end = launch_args
            .len()
            .saturating_sub(usize::from(prompt_arg.is_some()));
        let exec_args = launch_args
            .get(1..exec_args_end)
            .unwrap_or_default()
            .to_vec();

        args.push("exec".to_string());
        if continuation.is_some() {
            args.push("resume".to_string());
        }
        if capture_output {
            args.push("--json".to_string());
        }
        args.extend(exec_args);
        if let Some(continuation) = continuation {
            args.push(continuation.to_string());
        }
        if let Some(prompt_arg) = prompt_arg {
            args.push(prompt_arg);
        }
        Ok(args)
    }

    fn validate_command_args(&self, command_args: &[String]) -> Result<()> {
        let exec_index = command_args
            .iter()
            .position(|arg| arg == "exec")
            .ok_or_else(|| anyhow::anyhow!("built-in codex launch args are missing `exec`"))?;

        let top_level_args = &command_args[..exec_index];
        let exec_args = &command_args[exec_index + 1..];

        validate_help_surface(
            "codex",
            &["--help"],
            &[
                (
                    top_level_args.iter().any(|arg| arg == "--sandbox"),
                    "top-level sandbox flags",
                    &[FlagSupport::new(
                        "--sandbox",
                        &["--sandbox <SANDBOX_MODE>", "-s, --sandbox <SANDBOX_MODE>"],
                    )][..],
                ),
                (
                    top_level_args
                        .iter()
                        .any(|arg| arg == "--dangerously-bypass-approvals-and-sandbox"),
                    "top-level sandbox flags",
                    &[FlagSupport::new(
                        "--dangerously-bypass-approvals-and-sandbox",
                        &["--dangerously-bypass-approvals-and-sandbox"],
                    )][..],
                ),
                (
                    top_level_args.iter().any(|arg| arg == "--ask-for-approval"),
                    "top-level approval flags",
                    &[FlagSupport::new(
                        "--ask-for-approval",
                        &[
                            "--ask-for-approval <APPROVAL_POLICY>",
                            "-a, --ask-for-approval <APPROVAL_POLICY>",
                        ],
                    )][..],
                ),
                (
                    top_level_args.iter().any(|arg| arg == "--cd"),
                    "top-level working-directory flags",
                    &[FlagSupport::new("--cd", &["--cd <DIR>", "-C, --cd <DIR>"])][..],
                ),
                (
                    top_level_args.iter().any(|arg| arg == "--add-dir"),
                    "top-level writable-root flags",
                    &[FlagSupport::new("--add-dir", &["--add-dir <DIR>"])][..],
                ),
            ],
        )?;
        validate_help_surface(
            "codex",
            &["exec", "--help"],
            &[
                (
                    exec_args.iter().any(|arg| arg.starts_with("--model=")),
                    "exec model flags",
                    &[FlagSupport::new(
                        "--model",
                        &["--model <MODEL>", "-m, --model <MODEL>"],
                    )][..],
                ),
                (
                    exec_args.iter().any(|arg| arg == "-c"),
                    "exec config flags",
                    &[FlagSupport::new(
                        "-c",
                        &["-c, --config <key=value>", "--config <key=value>"],
                    )][..],
                ),
                (
                    exec_args.iter().any(|arg| arg == "--json"),
                    "exec machine-readable flags",
                    &[FlagSupport::new("--json", &["--json"])][..],
                ),
            ],
        )?;

        Ok(())
    }

    fn parse_capture_output(&self, raw_stdout: &str) -> Result<(String, Option<String>)> {
        let mut response_text = None;
        let mut continuation = None;

        for line in raw_stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            let value = match serde_json::from_str::<Value>(line) {
                Ok(value) => value,
                Err(_) => continue,
            };
            match value.get("type").and_then(Value::as_str) {
                Some("thread.started") => {
                    continuation = value
                        .get("thread_id")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
                Some("item.completed") => {
                    let item = value.get("item").unwrap_or(&Value::Null);
                    if item.get("type").and_then(Value::as_str) == Some("agent_message") {
                        response_text = item
                            .get("text")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .or(response_text);
                    }
                }
                _ => {}
            }
        }

        response_text
            .map(|text| (text, continuation))
            .ok_or_else(|| {
                anyhow!("codex did not emit an agent message while running in `--json` mode")
            })
    }

    fn is_invalid_resume_error(&self, message: &str) -> bool {
        let lower = message.to_ascii_lowercase();
        lower.contains("could not find thread")
            || lower.contains("no session found")
            || lower.contains("unknown session")
    }
}

impl BuiltinProviderAdapter for ClaudeProviderAdapter {
    fn models(&self) -> &'static [BuiltinModelCatalogEntry] {
        CLAUDE_MODELS
    }

    fn launch_command(&self) -> &'static str {
        "claude"
    }

    fn launch_args(&self, model: Option<&str>, reasoning: Option<&str>) -> Vec<String> {
        let mut args = vec!["-p".to_string()];
        if let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) {
            args.push(format!("--model={model}"));
        }
        if let Some(reasoning) = reasoning.map(str::trim).filter(|value| !value.is_empty()) {
            args.push(format!("--effort={reasoning}"));
        }
        args
    }

    fn transport(&self) -> PromptTransport {
        PromptTransport::Arg
    }

    fn prepare_command_args(
        &self,
        launch_args: &[String],
        _working_dir: Option<&Path>,
        context: BuiltinInvocationContext,
        transport: PromptTransport,
        capture_output: bool,
        continuation: Option<&str>,
    ) -> Result<Vec<String>> {
        let mut args = Vec::new();
        if context == BuiltinInvocationContext::Listen {
            args.push("--permission-mode=bypassPermissions".to_string());
            args.push("--verbose".to_string());
            args.push("--output-format=stream-json".to_string());
        }
        let prompt_arg = if transport == PromptTransport::Arg {
            launch_args.last().cloned()
        } else {
            None
        };
        let option_args_end = launch_args
            .len()
            .saturating_sub(usize::from(prompt_arg.is_some()));
        args.push("-p".to_string());
        if capture_output {
            args.push("--output-format=json".to_string());
        }
        args.extend(
            launch_args
                .get(1..option_args_end)
                .unwrap_or_default()
                .to_vec(),
        );
        if let Some(continuation) = continuation {
            args.push("--resume".to_string());
            args.push(continuation.to_string());
        }
        if let Some(prompt_arg) = prompt_arg {
            args.push(prompt_arg);
        }
        Ok(args)
    }

    fn validate_command_args(&self, command_args: &[String]) -> Result<()> {
        validate_help_surface(
            "claude",
            &["-p", "--help"],
            &[
                (
                    command_args.iter().any(|arg| arg == "-p"),
                    "print-mode flags",
                    &[FlagSupport::new("-p", &["-p, --print", "--print"])][..],
                ),
                (
                    command_args.iter().any(|arg| arg.starts_with("--model=")),
                    "model flags",
                    &[FlagSupport::new("--model", &["--model <model>"])][..],
                ),
                (
                    command_args.iter().any(|arg| arg.starts_with("--effort=")),
                    "effort flags",
                    &[FlagSupport::new("--effort", &["--effort <level>"])][..],
                ),
                (
                    command_args.iter().any(|arg| arg == "--verbose"),
                    "verbose flags",
                    &[FlagSupport::new("--verbose", &["--verbose"])][..],
                ),
                (
                    command_args
                        .iter()
                        .any(|arg| arg.starts_with("--output-format=")),
                    "output-format flags",
                    &[FlagSupport::new(
                        "--output-format",
                        &["--output-format <format>"],
                    )][..],
                ),
                (
                    command_args.iter().any(|arg| {
                        arg == "--permission-mode" || arg.starts_with("--permission-mode=")
                    }),
                    "permission-mode flags",
                    &[FlagSupport::new(
                        "--permission-mode",
                        &["--permission-mode <mode>"],
                    )][..],
                ),
            ],
        )
    }

    fn parse_capture_output(&self, raw_stdout: &str) -> Result<(String, Option<String>)> {
        let value: Value = serde_json::from_str(raw_stdout.trim())
            .context("failed to parse Claude `--output-format=json` response")?;
        let response_text = match value.get("result") {
            Some(Value::String(text)) => text.clone(),
            Some(value) => serde_json::to_string(value)
                .context("failed to serialize Claude structured result payload")?,
            None => bail!("Claude JSON response did not include a `result` field"),
        };
        let continuation = value
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        Ok((response_text, continuation))
    }

    fn is_invalid_resume_error(&self, message: &str) -> bool {
        let lower = message.to_ascii_lowercase();
        lower.contains("no conversation found with session id")
            || lower.contains("--resume requires a valid session id")
    }
}

static CODEX_PROVIDER: CodexProviderAdapter = CodexProviderAdapter;
static CLAUDE_PROVIDER: ClaudeProviderAdapter = ClaudeProviderAdapter;

pub fn builtin_provider_adapter(name: &str) -> Option<&'static dyn BuiltinProviderAdapter> {
    match normalize_agent_name(name).as_str() {
        "codex" => Some(&CODEX_PROVIDER),
        "claude" => Some(&CLAUDE_PROVIDER),
        _ => None,
    }
}

pub fn builtin_provider_names() -> &'static [&'static str] {
    &["codex", "claude"]
}

pub fn builtin_provider_model_keys(name: &str) -> Vec<&'static str> {
    builtin_provider_adapter(name)
        .map(|provider| provider.models().iter().map(|model| model.key).collect())
        .unwrap_or_default()
}

pub fn builtin_provider_reasoning_keys(provider: &str, model: Option<&str>) -> Vec<&'static str> {
    let Some(provider) = builtin_provider_adapter(provider) else {
        return Vec::new();
    };
    let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return Vec::new();
    };

    provider
        .models()
        .iter()
        .find(|entry| entry.key.eq_ignore_ascii_case(model))
        .map(|entry| {
            entry
                .reasoning_options
                .iter()
                .map(|option| option.key)
                .collect()
        })
        .unwrap_or_default()
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

fn git_stdout(root: &Path, args: &[&str]) -> Result<String> {
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

#[derive(Clone, Copy)]
struct FlagSupport {
    emitted_flag: &'static str,
    accepted_patterns: &'static [&'static str],
}

impl FlagSupport {
    const fn new(emitted_flag: &'static str, accepted_patterns: &'static [&'static str]) -> Self {
        Self {
            emitted_flag,
            accepted_patterns,
        }
    }
}

static HELP_OUTPUT_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn validate_help_surface(
    command: &str,
    help_args: &[&str],
    checks: &[(bool, &str, &[FlagSupport])],
) -> Result<()> {
    if checks.iter().all(|(required, _, _)| !*required) {
        return Ok(());
    }

    let help_output = cached_help_output(command, help_args)?;
    for (required, scope, flags) in checks.iter().copied() {
        if !required {
            continue;
        }
        for flag in flags {
            if flag
                .accepted_patterns
                .iter()
                .all(|pattern| !help_output.contains(*pattern))
            {
                bail!(
                    "installed `{}` {} does not advertise emitted flag `{}`; checked `{}`",
                    command,
                    scope,
                    flag.emitted_flag,
                    std::iter::once(command)
                        .chain(help_args.iter().copied())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
        }
    }

    Ok(())
}

fn cached_help_output(command: &str, help_args: &[&str]) -> Result<String> {
    let path_key = env::var("PATH").unwrap_or_default();
    let cache_key = std::iter::once(command)
        .chain(help_args.iter().copied())
        .chain(std::iter::once(path_key.as_str()))
        .collect::<Vec<_>>()
        .join("\u{0}");
    let cache = HELP_OUTPUT_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    {
        let cache_guard = cache
            .lock()
            .map_err(|_| anyhow::anyhow!("built-in CLI help cache lock is poisoned"))?;
        if let Some(output) = cache_guard.get(&cache_key) {
            return Ok(output.clone());
        }
    }

    let output = Command::new(command)
        .args(help_args)
        .output()
        .with_context(|| {
            format!(
                "failed to run `{}` while validating built-in CLI flags",
                std::iter::once(command)
                    .chain(help_args.iter().copied())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        })?;
    if !output.status.success() {
        bail!(
            "`{}` failed while validating built-in CLI flags: {}",
            std::iter::once(command)
                .chain(help_args.iter().copied())
                .collect::<Vec<_>>()
                .join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let help_output = String::from_utf8_lossy(&output.stdout).to_string();
    let mut cache_guard = cache
        .lock()
        .map_err(|_| anyhow::anyhow!("built-in CLI help cache lock is poisoned"))?;
    cache_guard.insert(cache_key, help_output.clone());
    Ok(help_output)
}

#[cfg(test)]
mod tests {
    use super::{
        BuiltinInvocationContext, BuiltinProviderAdapter, ClaudeProviderAdapter,
        CodexProviderAdapter,
    };
    use crate::config::PromptTransport;

    #[test]
    fn codex_capture_command_args_use_resume_json_and_preserve_prompt_order() {
        let adapter = CodexProviderAdapter;
        let args = adapter
            .prepare_command_args(
                &[
                    "exec".to_string(),
                    "--model=gpt-5.4".to_string(),
                    "-c".to_string(),
                    "reasoning.effort=\"high\"".to_string(),
                    "plan the work".to_string(),
                ],
                None,
                BuiltinInvocationContext::Planning,
                PromptTransport::Arg,
                true,
                Some("thread-1"),
            )
            .expect("codex args should render");

        assert_eq!(args[0], "--sandbox");
        assert!(args.contains(&"exec".to_string()));
        assert!(args.contains(&"resume".to_string()));
        assert!(args.contains(&"--json".to_string()));
        assert!(args.contains(&"thread-1".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("plan the work"));
    }

    #[test]
    fn codex_capture_output_extracts_last_agent_message_and_thread_id() {
        let adapter = CodexProviderAdapter;
        let (stdout, continuation) = adapter
            .parse_capture_output(
                r#"{"type":"thread.started","thread_id":"thread-123"}
{"type":"turn.started"}
{"type":"item.completed","item":{"type":"agent_message","text":"{\"questions\":[]}"}}"#,
            )
            .expect("codex output should parse");

        assert_eq!(stdout, r#"{"questions":[]}"#);
        assert_eq!(continuation.as_deref(), Some("thread-123"));
    }

    #[test]
    fn claude_capture_command_args_use_json_and_resume_flags() {
        let adapter = ClaudeProviderAdapter;
        let args = adapter
            .prepare_command_args(
                &[
                    "-p".to_string(),
                    "--model=haiku".to_string(),
                    "--effort=low".to_string(),
                    "plan the work".to_string(),
                ],
                None,
                BuiltinInvocationContext::Planning,
                PromptTransport::Arg,
                true,
                Some("session-123"),
            )
            .expect("claude args should render");

        assert_eq!(args[0], "-p");
        assert!(args.contains(&"--output-format=json".to_string()));
        assert!(args.contains(&"--resume".to_string()));
        assert!(args.contains(&"session-123".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("plan the work"));
    }

    #[test]
    fn claude_capture_output_extracts_result_and_session_id() {
        let adapter = ClaudeProviderAdapter;
        let (stdout, continuation) = adapter
            .parse_capture_output(
                r#"{"type":"result","subtype":"success","result":"{\"questions\":[]}","session_id":"session-123"}"#,
            )
            .expect("claude output should parse");

        assert_eq!(stdout, r#"{"questions":[]}"#);
        assert_eq!(continuation.as_deref(), Some("session-123"));
    }

    #[test]
    fn claude_invalid_resume_detection_is_narrow() {
        let adapter = ClaudeProviderAdapter;
        assert!(adapter.is_invalid_resume_error(
            "No conversation found with session ID: 550e8400-e29b-41d4-a716-446655440000"
        ));
        assert!(!adapter.is_invalid_resume_error("permission denied"));
    }
}

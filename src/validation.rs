use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::PlanningMeta;
use crate::managed_child::{ManagedChild, ManagedChildOutput, ManagedChildSettings};

const DEFAULT_VALIDATION_COMMAND_TIMEOUT: Duration = Duration::from_secs(1_800);
const VALIDATION_COMMAND_GRACEFUL_SHUTDOWN: Duration = Duration::from_secs(5);

/// Captures the command-selection precedence used for an effective validation profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ValidationProfileSource {
    CliOverride,
    RepoConfig,
    Heuristic,
}

impl ValidationProfileSource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::CliOverride => "cli_override",
            Self::RepoConfig => "repo_config",
            Self::Heuristic => "heuristic",
        }
    }
}

/// The ordered validation commands and metadata selected for the current repository root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ResolvedValidationProfile {
    pub(crate) commands: Vec<String>,
    pub(crate) source: ValidationProfileSource,
    #[serde(default)]
    pub(crate) profile_label: Option<String>,
}

impl ResolvedValidationProfile {
    /// Returns human-readable diagnostics describing the selected validation profile.
    pub(crate) fn diagnostics_lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "Validation profile source: {}",
            self.source.label()
        )];
        if let Some(label) = &self.profile_label {
            lines.push(format!("Validation profile label: {label}"));
        }
        lines.push(format!(
            "Validation commands: {}",
            self.commands.join(" && ")
        ));
        lines
    }
}

/// One validation command result captured from a workspace-local validation run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ValidationCommandRecord {
    pub(crate) command: String,
    pub(crate) exit_code: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

/// Resolves the effective validation profile for a repository root.
///
/// Returns an error when the repo config or inferred heuristics do not yield any validation
/// commands for the current repository.
pub(crate) fn resolve_validation_profile(
    root: &Path,
    planning_meta: &PlanningMeta,
    cli_override: &[String],
) -> Result<ResolvedValidationProfile> {
    if !cli_override.is_empty() {
        return Ok(ResolvedValidationProfile {
            commands: cli_override.to_vec(),
            source: ValidationProfileSource::CliOverride,
            profile_label: Some("cli override".to_string()),
        });
    }

    if !planning_meta.validation.commands.is_empty() {
        return Ok(ResolvedValidationProfile {
            commands: planning_meta.validation.commands.clone(),
            source: ValidationProfileSource::RepoConfig,
            profile_label: planning_meta.validation.profile_label(),
        });
    }

    if let Some(commands) = infer_validation_commands(root)? {
        return Ok(ResolvedValidationProfile {
            commands,
            source: ValidationProfileSource::Heuristic,
            profile_label: None,
        });
    }

    bail!(
        "no default validation command was inferred for `{}`; configure `{}/meta.json` `validation.commands` or pass explicit override commands",
        root.display(),
        crate::branding::PROJECT_DIR
    )
}

/// Executes validation commands inside the provided workspace and captures stdout/stderr for each.
///
/// Returns an error when a validation command cannot be launched.
pub(crate) fn run_validation_commands(
    workspace_path: &Path,
    commands: &[String],
) -> Result<Vec<ValidationCommandRecord>> {
    run_validation_commands_with_timeout(
        workspace_path,
        commands,
        DEFAULT_VALIDATION_COMMAND_TIMEOUT,
    )
}

fn run_validation_commands_with_timeout(
    workspace_path: &Path,
    commands: &[String],
    timeout: Duration,
) -> Result<Vec<ValidationCommandRecord>> {
    let mut records = Vec::with_capacity(commands.len());
    for command in commands {
        let mut child_command = Command::new("/bin/sh");
        child_command
            .arg("-lc")
            .arg(command)
            .current_dir(workspace_path);
        let child = ManagedChild::spawn(
            &mut child_command,
            ManagedChildOutput::Capture,
            ManagedChildOutput::Capture,
            ManagedChildSettings {
                timeout,
                graceful_shutdown: VALIDATION_COMMAND_GRACEFUL_SHUTDOWN,
            },
        )
        .with_context(|| format!("failed to run validation command `{command}`"))?;
        let output = child
            .wait_with_captured_output(|_| Ok(()), |_| Ok(()), |_| Ok(()))
            .with_context(|| format!("failed to run validation command `{command}`"))?;
        let mut stderr = output.stderr.unwrap_or_default();
        let timed_out = output.timeout.is_some();
        if let Some(timeout) = output.timeout {
            if !stderr.is_empty() && !stderr.ends_with('\n') {
                stderr.push('\n');
            }
            stderr.push_str(&format!(
                "validation command timed out after {}s (elapsed {}s); subprocess pid {} was terminated with {}\n",
                timeout.timeout_seconds(),
                timeout.elapsed_seconds(),
                timeout.pid,
                timeout.termination.label()
            ));
        }
        records.push(ValidationCommandRecord {
            command: command.clone(),
            exit_code: output
                .status
                .code()
                .unwrap_or(if timed_out { 124 } else { 1 }),
            stdout: output.stdout.unwrap_or_default(),
            stderr,
        });
    }
    Ok(records)
}

fn infer_validation_commands(root: &Path) -> Result<Option<Vec<String>>> {
    let makefile = root.join("Makefile");
    if makefile.is_file() {
        let contents = fs::read_to_string(&makefile)
            .with_context(|| format!("failed to read `{}`", makefile.display()))?;
        if makefile_has_target(&contents, "quality") {
            return Ok(Some(vec!["make quality".to_string()]));
        }
        if makefile_has_target(&contents, "all") {
            return Ok(Some(vec!["make all".to_string()]));
        }
    }

    if root.join("Cargo.toml").is_file() {
        return Ok(Some(vec!["cargo test".to_string()]));
    }

    if let Some(commands) = infer_javascript_validation_commands(root)? {
        return Ok(Some(commands));
    }

    Ok(None)
}

fn infer_javascript_validation_commands(root: &Path) -> Result<Option<Vec<String>>> {
    let roots = javascript_package_roots(root);
    if let Some(root_package) = load_package_json(root)? {
        if let Some(commands) = aggregate_commands_for_package_json(root, root, &root_package) {
            return Ok(Some(commands));
        }
    }

    for package_root in roots.iter().skip(1) {
        if let Some(package) = load_package_json(package_root)?
            && let Some(commands) = static_commands_for_package_json(root, package_root, &package)
        {
            return Ok(Some(commands_with_diff_check(commands)));
        }
    }

    if let Some(root_package) = load_package_json(root)?
        && let Some(commands) = static_commands_for_package_json(root, root, &root_package)
    {
        return Ok(Some(commands_with_diff_check(commands)));
    }

    Ok(None)
}

fn load_package_json(package_root: &Path) -> Result<Option<Value>> {
    let package_json = package_root.join("package.json");
    if !package_json.is_file() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&package_json)
        .with_context(|| format!("failed to read `{}`", package_json.display()))?;
    let package = serde_json::from_str::<Value>(&contents)
        .with_context(|| format!("failed to parse `{}`", package_json.display()))?;
    Ok(Some(package))
}

fn javascript_package_roots(root: &Path) -> Vec<PathBuf> {
    [
        PathBuf::new(),
        PathBuf::from("apps/web"),
        PathBuf::from("apps/experimental"),
        PathBuf::from("apps/frontend"),
        PathBuf::from("apps/client"),
        PathBuf::from("apps/admin"),
        PathBuf::from("app"),
        PathBuf::from("web"),
        PathBuf::from("frontend"),
        PathBuf::from("client"),
    ]
    .into_iter()
    .map(|relative| root.join(relative))
    .collect()
}

fn aggregate_commands_for_package_json(
    workspace_root: &Path,
    package_root: &Path,
    package: &Value,
) -> Option<Vec<String>> {
    let scripts = package.get("scripts")?.as_object()?;
    let manager = detect_package_manager(workspace_root, package);

    for script in ["quality", "validate", "verify"] {
        if scripts
            .get(script)
            .and_then(Value::as_str)
            .is_some_and(|body| is_safe_javascript_validation_script(script, body))
        {
            return Some(vec![package_script_command(
                manager,
                workspace_root,
                package_root,
                script,
            )]);
        }
    }

    None
}

fn static_commands_for_package_json(
    workspace_root: &Path,
    package_root: &Path,
    package: &Value,
) -> Option<Vec<String>> {
    let scripts = package.get("scripts")?.as_object()?;
    let manager = detect_package_manager(workspace_root, package);

    let check_script = scripts.get("check").and_then(Value::as_str);
    let typecheck_script = scripts
        .get("typecheck")
        .and_then(Value::as_str)
        .map(|body| ("typecheck", body))
        .or_else(|| {
            scripts
                .get("type-check")
                .and_then(Value::as_str)
                .map(|body| ("type-check", body))
        });
    let check_is_safe =
        check_script.is_some_and(|body| is_safe_javascript_validation_script("check", body));

    if let Some((script, body)) = typecheck_script
        && is_safe_javascript_validation_script(script, body)
        && !(check_is_safe && check_script.is_some_and(script_mentions_typecheck))
    {
        return Some(vec![package_script_command(
            manager,
            workspace_root,
            package_root,
            script,
        )]);
    }

    if check_is_safe {
        return Some(vec![package_script_command(
            manager,
            workspace_root,
            package_root,
            "check",
        )]);
    }

    if scripts
        .get("lint")
        .and_then(Value::as_str)
        .is_some_and(|body| is_safe_javascript_validation_script("lint", body))
    {
        return Some(vec![package_script_command(
            manager,
            workspace_root,
            package_root,
            "lint",
        )]);
    }

    if scripts
        .get("format:check")
        .and_then(Value::as_str)
        .is_some_and(|body| is_safe_javascript_validation_script("format:check", body))
    {
        return Some(vec![package_script_command(
            manager,
            workspace_root,
            package_root,
            "format:check",
        )]);
    }

    None
}

fn commands_with_diff_check(mut commands: Vec<String>) -> Vec<String> {
    commands.push("git diff --check".to_string());
    commands
}

fn script_mentions_typecheck(script: &str) -> bool {
    let normalized = script.to_ascii_lowercase();
    normalized.contains("typecheck")
        || normalized.contains("type-check")
        || normalized.contains("tsc")
}

fn is_safe_javascript_validation_script(script_name: &str, body: &str) -> bool {
    if script_mentions_unattended_unsafe_javascript_step(body) {
        return false;
    }
    if script_mentions_workspace_wide_javascript_runner(body) {
        return false;
    }

    matches!(
        script_name,
        "typecheck" | "type-check" | "lint" | "format:check"
    ) || script_mentions_static_javascript_validation(body)
}

fn script_mentions_unattended_unsafe_javascript_step(script: &str) -> bool {
    let normalized = script.to_ascii_lowercase();
    [
        "test",
        "vitest",
        "jest",
        "playwright",
        "cypress",
        "wdio",
        "webdriver",
        "storybook",
        "smoke",
        "e2e",
        "browser",
        "metamask",
        "rabby",
        "walletconnect",
        "wallet",
        "dev",
        "start",
        "serve",
        "preview",
    ]
    .iter()
    .any(|marker| script_contains_token(&normalized, marker))
}

fn script_mentions_workspace_wide_javascript_runner(script: &str) -> bool {
    let normalized = script.to_ascii_lowercase();
    [
        "turbo",
        "nx",
        "lerna",
        "lage",
        "moon",
        "pnpm -r",
        "pnpm --recursive",
        "yarn workspaces",
        "bun --filter",
        "bun -f",
    ]
    .iter()
    .any(|marker| script_contains_token(&normalized, marker))
}

fn script_mentions_static_javascript_validation(script: &str) -> bool {
    let normalized = script.to_ascii_lowercase();
    [
        "typecheck",
        "type-check",
        "tsc",
        "vue-tsc",
        "svelte-check",
        "check",
        "biome",
        "eslint",
        "prettier",
        "lint",
        "format:check",
        "oxlint",
        "knip",
    ]
    .iter()
    .any(|marker| script_contains_token(&normalized, marker))
}

fn script_contains_token(script: &str, marker: &str) -> bool {
    script.match_indices(marker).any(|(start, _)| {
        let before = script[..start].chars().next_back();
        let after = script[start + marker.len()..].chars().next();
        is_script_token_boundary(before) && is_script_token_boundary(after)
    })
}

fn is_script_token_boundary(character: Option<char>) -> bool {
    character.is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageManager {
    Bun,
    Pnpm,
    Yarn,
    Npm,
}

fn detect_package_manager(root: &Path, package: &Value) -> PackageManager {
    if let Some(manager) = package
        .get("packageManager")
        .and_then(Value::as_str)
        .map(str::trim)
    {
        if manager.starts_with("bun@") {
            return PackageManager::Bun;
        }
        if manager.starts_with("pnpm@") {
            return PackageManager::Pnpm;
        }
        if manager.starts_with("yarn@") {
            return PackageManager::Yarn;
        }
        if manager.starts_with("npm@") {
            return PackageManager::Npm;
        }
    }

    if root.join("bun.lock").is_file() || root.join("bun.lockb").is_file() {
        PackageManager::Bun
    } else if root.join("pnpm-lock.yaml").is_file() {
        PackageManager::Pnpm
    } else if root.join("yarn.lock").is_file() {
        PackageManager::Yarn
    } else {
        PackageManager::Npm
    }
}

fn package_script_command(
    manager: PackageManager,
    workspace_root: &Path,
    package_root: &Path,
    script: &str,
) -> String {
    let relative_root = package_root
        .strip_prefix(workspace_root)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty());

    match (manager, relative_root) {
        (PackageManager::Bun, Some(relative)) => {
            format!(
                "bun run --cwd {} {}",
                shell_quote_path(relative),
                shell_quote(script)
            )
        }
        (PackageManager::Bun, None) => format!("bun run {}", shell_quote(script)),
        (PackageManager::Pnpm, Some(relative)) => {
            format!(
                "pnpm --dir {} run {}",
                shell_quote_path(relative),
                shell_quote(script)
            )
        }
        (PackageManager::Pnpm, None) => format!("pnpm run {}", shell_quote(script)),
        (PackageManager::Yarn, Some(relative)) => {
            format!(
                "yarn --cwd {} run {}",
                shell_quote_path(relative),
                shell_quote(script)
            )
        }
        (PackageManager::Yarn, None) => format!("yarn run {}", shell_quote(script)),
        (PackageManager::Npm, Some(relative)) => {
            format!(
                "npm --prefix {} run {}",
                shell_quote_path(relative),
                shell_quote(script)
            )
        }
        (PackageManager::Npm, None) => format!("npm run {}", shell_quote(script)),
    }
}

fn shell_quote_path(path: &Path) -> String {
    shell_quote(&path.display().to_string())
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/'))
    {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}

fn makefile_has_target(contents: &str, target: &str) -> bool {
    contents.lines().any(|line| {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.starts_with('\t') || trimmed.is_empty() {
            return false;
        }

        trimmed
            .split_once(':')
            .map(|(name, _)| name.trim() == target)
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::{
        ValidationProfileSource, resolve_validation_profile, run_validation_commands_with_timeout,
    };
    use crate::config::{PlanningMeta, PlanningValidationSettings};
    use anyhow::Result;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn resolver_prefers_cli_override() -> Result<()> {
        let temp = tempdir()?;
        let profile = resolve_validation_profile(
            temp.path(),
            &PlanningMeta::default(),
            &["cargo test -p cli".to_string()],
        )?;

        assert_eq!(profile.commands, vec!["cargo test -p cli"]);
        assert_eq!(profile.source, ValidationProfileSource::CliOverride);
        assert_eq!(profile.profile_label.as_deref(), Some("cli override"));
        Ok(())
    }

    #[test]
    fn resolver_uses_repo_config_before_heuristics() -> Result<()> {
        let temp = tempdir()?;
        std::fs::write(
            temp.path().join("Makefile"),
            ".PHONY: quality\nquality:\n\tcargo test\n",
        )?;
        let profile = resolve_validation_profile(
            temp.path(),
            &PlanningMeta {
                validation: PlanningValidationSettings {
                    commands: vec!["cargo test --workspace".to_string()],
                    repair_attempts: Some(3),
                    profile: Some("workspace".to_string()),
                },
                ..PlanningMeta::default()
            },
            &[],
        )?;

        assert_eq!(profile.commands, vec!["cargo test --workspace"]);
        assert_eq!(profile.source, ValidationProfileSource::RepoConfig);
        assert_eq!(profile.profile_label.as_deref(), Some("workspace"));
        Ok(())
    }

    #[test]
    fn resolver_infers_make_quality_then_all_then_cargo_test() -> Result<()> {
        let quality = tempdir()?;
        std::fs::write(
            quality.path().join("Makefile"),
            ".PHONY: all quality\nall:\n\tcargo test\nquality:\n\tcargo test\n",
        )?;
        assert_eq!(
            resolve_validation_profile(quality.path(), &PlanningMeta::default(), &[])?.commands,
            vec!["make quality"]
        );

        let all = tempdir()?;
        std::fs::write(
            all.path().join("Makefile"),
            ".PHONY: all\nall:\n\tcargo test\n",
        )?;
        assert_eq!(
            resolve_validation_profile(all.path(), &PlanningMeta::default(), &[])?.commands,
            vec!["make all"]
        );

        let cargo = tempdir()?;
        std::fs::write(
            cargo.path().join("Cargo.toml"),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )?;
        assert_eq!(
            resolve_validation_profile(cargo.path(), &PlanningMeta::default(), &[])?.commands,
            vec!["cargo test"]
        );

        Ok(())
    }

    #[test]
    fn resolver_infers_static_javascript_validation_without_tests() -> Result<()> {
        let temp = tempdir()?;
        std::fs::write(temp.path().join("bun.lock"), "")?;
        std::fs::write(
            temp.path().join("package.json"),
            r#"{
              "packageManager": "bun@1.3.3",
              "scripts": {
                "typecheck": "tsc --noEmit",
                "check": "biome check",
                "test": "bun test"
              }
            }"#,
        )?;

        assert_eq!(
            resolve_validation_profile(temp.path(), &PlanningMeta::default(), &[])?.commands,
            vec!["bun run typecheck", "git diff --check"]
        );
        Ok(())
    }

    #[test]
    fn resolver_skips_javascript_test_only_scripts() -> Result<()> {
        let temp = tempdir()?;
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"scripts":{"test":"vitest run"}}"#,
        )?;

        let error = resolve_validation_profile(temp.path(), &PlanningMeta::default(), &[])
            .expect_err("test-only JavaScript packages should require explicit validation config");
        assert!(
            error
                .to_string()
                .contains("no default validation command was inferred")
        );
        Ok(())
    }

    #[test]
    fn resolver_infers_static_javascript_quality_script() -> Result<()> {
        let temp = tempdir()?;
        std::fs::write(temp.path().join("bun.lock"), "")?;
        std::fs::write(
            temp.path().join("package.json"),
            r#"{
              "packageManager": "bun@1.3.3",
              "scripts": {
                "quality": "bun run typecheck && bun run lint",
                "typecheck": "tsc --noEmit",
                "lint": "eslint ."
              }
            }"#,
        )?;

        assert_eq!(
            resolve_validation_profile(temp.path(), &PlanningMeta::default(), &[])?.commands,
            vec!["bun run quality"]
        );
        Ok(())
    }

    #[test]
    fn resolver_skips_javascript_quality_when_it_runs_tests() -> Result<()> {
        let temp = tempdir()?;
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"scripts":{"quality":"bun test"}}"#,
        )?;

        let error = resolve_validation_profile(temp.path(), &PlanningMeta::default(), &[])
            .expect_err("test-backed JavaScript quality scripts should require explicit config");
        assert!(
            error
                .to_string()
                .contains("no default validation command was inferred")
        );
        Ok(())
    }

    #[test]
    fn resolver_skips_workspace_wide_javascript_quality_script() -> Result<()> {
        let temp = tempdir()?;
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"scripts":{"quality":"turbo run check"}}"#,
        )?;

        let error = resolve_validation_profile(temp.path(), &PlanningMeta::default(), &[])
            .expect_err("workspace-wide JavaScript quality scripts should require explicit config");
        assert!(
            error
                .to_string()
                .contains("no default validation command was inferred")
        );
        Ok(())
    }

    #[test]
    fn resolver_skips_browser_check_scripts_and_uses_static_fallback() -> Result<()> {
        let temp = tempdir()?;
        std::fs::write(temp.path().join("bun.lock"), "")?;
        let app_dir = temp.path().join("apps/web");
        std::fs::create_dir_all(&app_dir)?;
        std::fs::write(
            app_dir.join("package.json"),
            r#"{
              "scripts": {
                "check": "playwright test",
                "lint": "eslint ."
              }
            }"#,
        )?;

        assert_eq!(
            resolve_validation_profile(temp.path(), &PlanningMeta::default(), &[])?.commands,
            vec!["bun run --cwd apps/web lint", "git diff --check"]
        );
        Ok(())
    }

    #[test]
    fn resolver_skips_javascript_browser_check_only_scripts() -> Result<()> {
        let temp = tempdir()?;
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"scripts":{"check":"playwright test"}}"#,
        )?;

        let error = resolve_validation_profile(temp.path(), &PlanningMeta::default(), &[])
            .expect_err("browser-only JavaScript check scripts should require explicit config");
        assert!(
            error
                .to_string()
                .contains("no default validation command was inferred")
        );
        Ok(())
    }

    #[test]
    fn resolver_skips_workspace_wide_root_check_and_uses_nested_static_script() -> Result<()> {
        let temp = tempdir()?;
        std::fs::write(temp.path().join("bun.lock"), "")?;
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"scripts":{"check":"turbo run check","typecheck":"turbo run typecheck"}}"#,
        )?;
        let app_dir = temp.path().join("apps/web");
        std::fs::create_dir_all(&app_dir)?;
        std::fs::write(
            app_dir.join("package.json"),
            r#"{"scripts":{"typecheck":"tsc --noEmit","check":"biome check"}}"#,
        )?;

        assert_eq!(
            resolve_validation_profile(temp.path(), &PlanningMeta::default(), &[])?.commands,
            vec!["bun run --cwd apps/web typecheck", "git diff --check"]
        );
        Ok(())
    }

    #[test]
    fn resolver_infers_nested_frontend_package_validation() -> Result<()> {
        let temp = tempdir()?;
        std::fs::write(temp.path().join("bun.lock"), "")?;
        let app_dir = temp.path().join("apps/web");
        std::fs::create_dir_all(&app_dir)?;
        std::fs::write(
            app_dir.join("package.json"),
            r#"{"scripts":{"check":"biome check","test":"bun test"}}"#,
        )?;

        assert_eq!(
            resolve_validation_profile(temp.path(), &PlanningMeta::default(), &[])?.commands,
            vec!["bun run --cwd apps/web check", "git diff --check"]
        );
        Ok(())
    }

    #[test]
    fn validation_command_timeout_records_failed_command() -> Result<()> {
        let temp = tempdir()?;
        let records = run_validation_commands_with_timeout(
            temp.path(),
            &["sleep 5".to_string()],
            Duration::from_millis(100),
        )?;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].command, "sleep 5");
        assert_eq!(records[0].exit_code, 124);
        assert!(
            records[0].stderr.contains("validation command timed out"),
            "{:?}",
            records[0]
        );
        Ok(())
    }
}

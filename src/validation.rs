use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::PlanningMeta;

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
    let mut records = Vec::with_capacity(commands.len());
    for command in commands {
        let output = Command::new("/bin/sh")
            .arg("-lc")
            .arg(command)
            .current_dir(workspace_path)
            .output()
            .with_context(|| format!("failed to run validation command `{command}`"))?;
        records.push(ValidationCommandRecord {
            command: command.clone(),
            exit_code: output.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
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

    Ok(None)
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
    use super::{ValidationProfileSource, resolve_validation_profile};
    use crate::config::{PlanningMeta, PlanningValidationSettings};
    use anyhow::Result;
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
}

use std::path::Path;

use anyhow::{Result, bail};

use crate::config::{AppConfig, DEFAULT_COMMAND_NAME, DEFAULT_STATE_DIRECTORY, PlanningMeta};
use crate::fs::PlanningPaths;

/// Resolved effective branding and layout settings for the current command invocation.
///
/// Built from explicit overrides, repo-scoped defaults, install-scoped defaults, and
/// compatibility fallbacks. All consumers should use these resolved values instead of
/// hardcoding `"meta"` or `".metastack"`.
#[derive(Debug, Clone)]
pub struct EffectiveBranding {
    /// The effective CLI command name (e.g. `"meta"`, `"intuition"`, `"int"`).
    pub command_name: String,
    /// The effective repo-local state directory name (e.g. `".metastack"`, `".intuition"`).
    pub state_directory: String,
}

impl Default for EffectiveBranding {
    fn default() -> Self {
        Self {
            command_name: DEFAULT_COMMAND_NAME.to_string(),
            state_directory: DEFAULT_STATE_DIRECTORY.to_string(),
        }
    }
}

impl EffectiveBranding {
    /// Resolves effective branding using the standard precedence chain:
    /// explicit override -> repo default -> install default -> compatibility default.
    pub fn resolve(planning_meta: &PlanningMeta, app_config: &AppConfig) -> Self {
        Self {
            command_name: planning_meta.effective_command_name(app_config),
            state_directory: planning_meta.effective_state_directory(app_config),
        }
    }

    /// Resolves effective branding with explicit overrides that take highest precedence.
    ///
    /// Use this when CLI flags or environment variables provide explicit values that
    /// should override both repo-scoped and install-scoped defaults.
    pub fn resolve_with_overrides(
        planning_meta: &PlanningMeta,
        app_config: &AppConfig,
        command_name_override: Option<&str>,
        state_directory_override: Option<&str>,
    ) -> Self {
        let base = Self::resolve(planning_meta, app_config);
        Self {
            command_name: command_name_override
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or(base.command_name),
            state_directory: state_directory_override
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or(base.state_directory),
        }
    }

    /// Returns whether the effective state directory differs from the default `.metastack`.
    pub fn has_custom_state_directory(&self) -> bool {
        self.state_directory != DEFAULT_STATE_DIRECTORY
    }

    /// Returns whether the effective command name differs from the default `meta`.
    pub fn has_custom_command_name(&self) -> bool {
        self.command_name != DEFAULT_COMMAND_NAME
    }

    /// Constructs `PlanningPaths` using the effective state directory name.
    pub fn planning_paths(&self, root: &Path) -> PlanningPaths {
        PlanningPaths::with_state_dir(root, &self.state_directory)
    }

    /// Validates that the effective state directory resolves under the repository root
    /// and does not escape it.
    ///
    /// Returns an error when the resolved path would land outside `repo_root`.
    pub fn validate_state_dir_containment(&self, repo_root: &Path) -> Result<()> {
        let resolved = repo_root.join(&self.state_directory);

        // Ensure the resolved directory is a child of repo_root by checking the
        // canonical prefix relationship. For paths that don't exist yet we check
        // component-level containment instead.
        if let (Ok(canon_root), Ok(canon_resolved)) =
            (repo_root.canonicalize(), resolved.canonicalize())
        {
            if !canon_resolved.starts_with(&canon_root) {
                bail!(
                    "state directory `{}` resolves outside the repository root `{}`",
                    self.state_directory,
                    repo_root.display()
                );
            }
        } else {
            // Path doesn't exist yet - validate structurally
            let dir_name = &self.state_directory;
            if dir_name.contains('/') || dir_name.contains(std::path::MAIN_SEPARATOR) {
                bail!(
                    "state directory `{dir_name}` must be a single directory name without path separators"
                );
            }
            if dir_name == "." || dir_name == ".." {
                bail!("state directory cannot be `.` or `..`");
            }
        }

        Ok(())
    }

    /// Formats a user-facing reference to the repo metadata file path relative to the
    /// effective state directory.
    pub fn meta_json_display(&self) -> String {
        format!("{}/meta.json", self.state_directory)
    }

    /// Formats a user-facing reference to the backlog directory relative to the
    /// effective state directory.
    pub fn backlog_display(&self) -> String {
        format!("{}/backlog", self.state_directory)
    }

    /// Formats a user-facing reference to the codebase directory relative to the
    /// effective state directory.
    pub fn codebase_display(&self) -> String {
        format!("{}/codebase", self.state_directory)
    }
}

/// Discovers the effective branding for a repository root by loading available config.
///
/// This is a convenience helper for callsites that don't already have resolved config
/// available. It loads the install-scoped `AppConfig` and repo-scoped `PlanningMeta`
/// to determine the effective state directory, trying the configured directory first
/// and falling back to `.metastack`. When both the configured and default state
/// directories are missing, it probes the repository root for any dot-directory
/// containing `meta.json` to handle post-migration scenarios where the install-scoped
/// config hasn't been updated.
pub fn discover_effective_branding(root: &Path) -> EffectiveBranding {
    let app_config = AppConfig::load().unwrap_or_default();
    let effective_state_dir = app_config
        .branding
        .state_directory
        .as_deref()
        .unwrap_or(DEFAULT_STATE_DIRECTORY);

    // Try loading from the configured state dir if its directory exists
    if root.join(effective_state_dir).is_dir() {
        if let Ok(meta) = PlanningMeta::load_from_state_dir(root, effective_state_dir) {
            return EffectiveBranding::resolve(&meta, &app_config);
        }
    }

    // If a non-default dir was configured but missing, try the default
    if effective_state_dir != DEFAULT_STATE_DIRECTORY && root.join(DEFAULT_STATE_DIRECTORY).is_dir()
    {
        if let Ok(meta) = PlanningMeta::load(root) {
            return EffectiveBranding::resolve(&meta, &app_config);
        }
    }

    // Neither configured nor default dir exists - probe for a migrated state directory
    if let Some(meta) = probe_migrated_state_dir(root) {
        return EffectiveBranding::resolve(&meta, &app_config);
    }

    EffectiveBranding::resolve(&PlanningMeta::default(), &app_config)
}

/// Probes the repository root for a dot-directory containing `meta.json` with
/// `branding.state_directory` set. This handles the post-migration case where
/// `.metastack` has been renamed but the install-scoped config hasn't been updated.
///
/// Returns `Some(meta)` if exactly one such directory is found, `None` otherwise.
fn probe_migrated_state_dir(root: &Path) -> Option<PlanningMeta> {
    let entries = std::fs::read_dir(root).ok()?;
    let mut found: Option<PlanningMeta> = None;

    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // Only consider dot-directories that aren't the default
        if !name_str.starts_with('.') || name_str == DEFAULT_STATE_DIRECTORY {
            continue;
        }
        if !entry.path().join("meta.json").is_file() {
            continue;
        }
        if let Ok(meta) = PlanningMeta::load_from_state_dir(root, &name_str) {
            if meta.branding.state_directory.is_some() {
                if found.is_some() {
                    // Multiple candidates - ambiguous, bail out
                    return None;
                }
                found = Some(meta);
            }
        }
    }

    found
}

/// Returns the effective `PlanningPaths` for a repository root by discovering branding.
///
/// This is the recommended entry point for callsites that need `PlanningPaths` but don't
/// have resolved branding available. It's equivalent to:
/// ```ignore
/// discover_effective_branding(root).planning_paths(root)
/// ```
pub fn effective_planning_paths(root: &Path) -> PlanningPaths {
    discover_effective_branding(root).planning_paths(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, InstallBrandingSettings, PlanningMeta, RepoBrandingSettings};

    #[test]
    fn default_branding_uses_meta_and_metastack() {
        let branding = EffectiveBranding::default();
        assert_eq!(branding.command_name, "meta");
        assert_eq!(branding.state_directory, ".metastack");
        assert!(!branding.has_custom_command_name());
        assert!(!branding.has_custom_state_directory());
    }

    #[test]
    fn resolve_falls_back_to_defaults_when_no_overrides() {
        let meta = PlanningMeta::default();
        let config = AppConfig::default();
        let branding = EffectiveBranding::resolve(&meta, &config);
        assert_eq!(branding.command_name, "meta");
        assert_eq!(branding.state_directory, ".metastack");
    }

    #[test]
    fn resolve_uses_install_defaults() {
        let meta = PlanningMeta::default();
        let config = AppConfig {
            branding: InstallBrandingSettings {
                command_name: Some("intuition".to_string()),
                state_directory: Some(".intuition".to_string()),
            },
            ..Default::default()
        };
        let branding = EffectiveBranding::resolve(&meta, &config);
        assert_eq!(branding.command_name, "intuition");
        assert_eq!(branding.state_directory, ".intuition");
        assert!(branding.has_custom_command_name());
        assert!(branding.has_custom_state_directory());
    }

    #[test]
    fn resolve_repo_overrides_install() {
        let meta = PlanningMeta {
            branding: RepoBrandingSettings {
                command_name: Some("int".to_string()),
                state_directory: Some(".int".to_string()),
            },
            ..Default::default()
        };
        let config = AppConfig {
            branding: InstallBrandingSettings {
                command_name: Some("intuition".to_string()),
                state_directory: Some(".intuition".to_string()),
            },
            ..Default::default()
        };
        let branding = EffectiveBranding::resolve(&meta, &config);
        assert_eq!(branding.command_name, "int");
        assert_eq!(branding.state_directory, ".int");
    }

    #[test]
    fn resolve_with_overrides_takes_highest_precedence() {
        let meta = PlanningMeta {
            branding: RepoBrandingSettings {
                command_name: Some("int".to_string()),
                state_directory: Some(".int".to_string()),
            },
            ..Default::default()
        };
        let config = AppConfig::default();
        let branding = EffectiveBranding::resolve_with_overrides(
            &meta,
            &config,
            Some("custom"),
            Some(".custom"),
        );
        assert_eq!(branding.command_name, "custom");
        assert_eq!(branding.state_directory, ".custom");
    }

    #[test]
    fn resolve_with_overrides_ignores_empty_overrides() {
        let meta = PlanningMeta {
            branding: RepoBrandingSettings {
                command_name: Some("int".to_string()),
                state_directory: None,
            },
            ..Default::default()
        };
        let config = AppConfig::default();
        let branding =
            EffectiveBranding::resolve_with_overrides(&meta, &config, Some("  "), Some(""));
        assert_eq!(branding.command_name, "int");
        assert_eq!(branding.state_directory, ".metastack");
    }

    #[test]
    fn planning_paths_uses_effective_state_dir() {
        let branding = EffectiveBranding {
            command_name: "intuition".to_string(),
            state_directory: ".intuition".to_string(),
        };
        let paths = branding.planning_paths(Path::new("/tmp/repo"));
        assert_eq!(paths.metastack_dir, Path::new("/tmp/repo/.intuition"));
        assert_eq!(paths.backlog_dir, Path::new("/tmp/repo/.intuition/backlog"));
        assert_eq!(paths.state_dir_name(), ".intuition");
    }

    #[test]
    fn validate_containment_rejects_path_separators() {
        let branding = EffectiveBranding {
            command_name: "meta".to_string(),
            state_directory: "../escape".to_string(),
        };
        let result = branding.validate_state_dir_containment(Path::new("/tmp/repo"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path separators"));
    }

    #[test]
    fn validate_containment_rejects_dot_dot() {
        let branding = EffectiveBranding {
            command_name: "meta".to_string(),
            state_directory: "..".to_string(),
        };
        let result = branding.validate_state_dir_containment(Path::new("/tmp/repo"));
        assert!(result.is_err());
    }

    #[test]
    fn validate_containment_accepts_valid_names() {
        let branding = EffectiveBranding {
            command_name: "meta".to_string(),
            state_directory: ".intuition".to_string(),
        };
        // This won't error because the path doesn't exist but structurally it's valid
        let result = branding.validate_state_dir_containment(Path::new("/tmp/repo"));
        assert!(result.is_ok());
    }

    #[test]
    fn meta_json_display_uses_effective_state_dir() {
        let branding = EffectiveBranding {
            command_name: "intuition".to_string(),
            state_directory: ".intuition".to_string(),
        };
        assert_eq!(branding.meta_json_display(), ".intuition/meta.json");
    }

    #[test]
    fn backlog_display_uses_effective_state_dir() {
        let branding = EffectiveBranding {
            command_name: "intuition".to_string(),
            state_directory: ".intuition".to_string(),
        };
        assert_eq!(branding.backlog_display(), ".intuition/backlog");
    }

    #[test]
    fn probe_finds_migrated_state_dir() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        // Set up a migrated state directory with branding
        let dir = root.join(".intuition");
        std::fs::create_dir_all(&dir).unwrap();
        let mut meta = PlanningMeta::default();
        meta.branding.state_directory = Some(".intuition".to_string());
        meta.save_to_state_dir(root, ".intuition").unwrap();

        let found = probe_migrated_state_dir(root);
        assert!(found.is_some());
        assert_eq!(
            found.unwrap().branding.state_directory.as_deref(),
            Some(".intuition")
        );
    }

    #[test]
    fn probe_returns_none_when_multiple_candidates() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        // Set up two migrated directories - should be ambiguous
        for name in &[".intuition", ".int"] {
            std::fs::create_dir_all(root.join(name)).unwrap();
            let mut meta = PlanningMeta::default();
            meta.branding.state_directory = Some(name.to_string());
            meta.save_to_state_dir(root, name).unwrap();
        }

        let found = probe_migrated_state_dir(root);
        assert!(found.is_none());
    }

    #[test]
    fn probe_ignores_dot_dirs_without_branding() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        // Set up a dot-directory with meta.json but no branding
        let dir = root.join(".other");
        std::fs::create_dir_all(&dir).unwrap();
        let meta = PlanningMeta::default();
        meta.save_to_state_dir(root, ".other").unwrap();

        let found = probe_migrated_state_dir(root);
        assert!(found.is_none());
    }
}

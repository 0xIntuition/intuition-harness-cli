use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::config::{DEFAULT_STATE_DIRECTORY, PlanningMeta};
use crate::fs::PlanningPaths;

/// Result of a migration operation.
#[derive(Debug, Clone, Serialize)]
pub struct MigrateReport {
    pub root: PathBuf,
    pub source: String,
    pub destination: String,
    pub status: MigrateStatus,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub moved_entries: Vec<String>,
}

/// Status of a migration operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrateStatus {
    /// The source directory was migrated to the destination.
    Migrated,
    /// The destination already exists and contains state; no migration needed.
    AlreadyMigrated,
    /// The source directory does not exist; nothing to migrate.
    NoSource,
}

impl MigrateReport {
    /// Renders a human-readable summary of the migration result.
    pub fn render(&self) -> String {
        match self.status {
            MigrateStatus::Migrated => {
                let mut lines = vec![format!(
                    "Migrated `{}` -> `{}` in `{}`",
                    self.source,
                    self.destination,
                    self.root.display(),
                )];
                if !self.moved_entries.is_empty() {
                    lines.push(format!("  {} entries preserved.", self.moved_entries.len()));
                }
                lines.join("\n")
            }
            MigrateStatus::AlreadyMigrated => {
                format!(
                    "Already migrated: `{}` exists in `{}`.",
                    self.destination,
                    self.root.display(),
                )
            }
            MigrateStatus::NoSource => {
                format!(
                    "No source directory `{}` found in `{}`; nothing to migrate.",
                    self.source,
                    self.root.display(),
                )
            }
        }
    }
}

/// Migrates repo-local state from a source directory to a destination directory.
///
/// The migration:
/// 1. Validates both source and destination are repository-contained single directory names.
/// 2. Returns `AlreadyMigrated` if the destination already has a `meta.json`.
/// 3. Returns `NoSource` if the source directory does not exist.
/// 4. Moves the source directory to the destination, preserving all contents.
/// 5. Updates the `branding.state_directory` field in the destination `meta.json`.
///
/// The migration is rerunnable: re-running after a successful migration returns
/// `AlreadyMigrated` without modifying anything.
pub fn migrate_state_root(
    root: &Path,
    source_dir_name: &str,
    destination_dir_name: &str,
) -> Result<MigrateReport> {
    // Validate inputs
    validate_dir_name(source_dir_name, "source")?;
    validate_dir_name(destination_dir_name, "destination")?;

    if source_dir_name == destination_dir_name {
        bail!("source and destination directory names are the same: `{source_dir_name}`");
    }

    let source = root.join(source_dir_name);
    let destination = root.join(destination_dir_name);

    // Check if already migrated
    let dest_meta = destination.join("meta.json");
    if dest_meta.is_file() {
        return Ok(MigrateReport {
            root: root.to_path_buf(),
            source: source_dir_name.to_string(),
            destination: destination_dir_name.to_string(),
            status: MigrateStatus::AlreadyMigrated,
            moved_entries: Vec::new(),
        });
    }

    // Check source exists
    if !source.is_dir() {
        return Ok(MigrateReport {
            root: root.to_path_buf(),
            source: source_dir_name.to_string(),
            destination: destination_dir_name.to_string(),
            status: MigrateStatus::NoSource,
            moved_entries: Vec::new(),
        });
    }

    // Ensure destination doesn't exist as a non-directory
    if destination.exists() && !destination.is_dir() {
        bail!(
            "destination `{}` exists and is not a directory",
            destination.display()
        );
    }

    // If destination directory exists but is empty or has no meta.json, proceed with merge
    if destination.is_dir() {
        // Check it's empty before merging
        let has_entries = fs::read_dir(&destination)
            .with_context(|| format!("failed to read `{}`", destination.display()))?
            .next()
            .is_some();
        if has_entries {
            bail!(
                "destination `{}` already exists and is non-empty but has no `meta.json`; refusing to overwrite",
                destination.display()
            );
        }
        // Remove empty destination so rename succeeds
        fs::remove_dir(&destination)
            .with_context(|| format!("failed to remove empty `{}`", destination.display()))?;
    }

    // Collect entry names before rename for the report
    let moved_entries: Vec<String> = fs::read_dir(&source)
        .with_context(|| format!("failed to read `{}`", source.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();

    // Rename source -> destination
    fs::rename(&source, &destination).with_context(|| {
        format!(
            "failed to rename `{}` to `{}`",
            source.display(),
            destination.display()
        )
    })?;

    // Update branding.state_directory in the migrated meta.json
    let new_paths = PlanningPaths::with_state_dir(root, destination_dir_name);
    let meta_path = new_paths.meta_path();
    if meta_path.is_file() {
        let mut planning_meta = PlanningMeta::load_from_state_dir(root, destination_dir_name)
            .context("failed to load migrated metadata")?;
        planning_meta.branding.state_directory = Some(destination_dir_name.to_string());
        planning_meta
            .save_to_state_dir(root, destination_dir_name)
            .context("failed to update migrated metadata")?;
    }

    Ok(MigrateReport {
        root: root.to_path_buf(),
        source: source_dir_name.to_string(),
        destination: destination_dir_name.to_string(),
        status: MigrateStatus::Migrated,
        moved_entries,
    })
}

/// Validates that a directory name is a single component without path separators.
fn validate_dir_name(name: &str, label: &str) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("{label} directory name cannot be empty");
    }
    if trimmed.contains('/') || trimmed.contains(std::path::MAIN_SEPARATOR) {
        bail!("{label} directory name must not contain path separators: `{name}`");
    }
    if trimmed == "." || trimmed == ".." {
        bail!("{label} directory name cannot be `.` or `..`");
    }
    Ok(())
}

/// Convenience function: migrate from default `.metastack` to a branded state root.
#[allow(dead_code)]
pub fn migrate_from_default(root: &Path, destination_dir_name: &str) -> Result<MigrateReport> {
    migrate_state_root(root, DEFAULT_STATE_DIRECTORY, destination_dir_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn migrate_succeeds_with_existing_source() -> Result<()> {
        let temp = tempdir()?;
        let root = temp.path();

        // Create source .metastack with meta.json
        let source = root.join(".metastack");
        fs::create_dir_all(source.join("backlog").join("ENG-1"))?;
        fs::write(source.join("meta.json"), "{}")?;
        fs::write(
            source.join("backlog").join("ENG-1").join("index.md"),
            "# ENG-1",
        )?;

        let report = migrate_state_root(root, ".metastack", ".intuition")?;
        assert_eq!(report.status, MigrateStatus::Migrated);
        assert!(!report.moved_entries.is_empty());

        // Verify destination exists with content
        assert!(root.join(".intuition").join("meta.json").is_file());
        assert!(
            root.join(".intuition")
                .join("backlog")
                .join("ENG-1")
                .join("index.md")
                .is_file()
        );

        // Verify source is gone
        assert!(!root.join(".metastack").exists());

        // Verify meta.json was updated with branding
        let meta = PlanningMeta::load_from_state_dir(root, ".intuition")?;
        assert_eq!(meta.branding.state_directory.as_deref(), Some(".intuition"));

        Ok(())
    }

    #[test]
    fn migrate_returns_already_migrated_when_destination_has_meta_json() -> Result<()> {
        let temp = tempdir()?;
        let root = temp.path();

        // Create both source and destination
        fs::create_dir_all(root.join(".metastack"))?;
        fs::write(root.join(".metastack").join("meta.json"), "{}")?;
        fs::create_dir_all(root.join(".intuition"))?;
        fs::write(root.join(".intuition").join("meta.json"), "{}")?;

        let report = migrate_state_root(root, ".metastack", ".intuition")?;
        assert_eq!(report.status, MigrateStatus::AlreadyMigrated);

        Ok(())
    }

    #[test]
    fn migrate_returns_no_source_when_source_missing() -> Result<()> {
        let temp = tempdir()?;
        let root = temp.path();

        let report = migrate_state_root(root, ".metastack", ".intuition")?;
        assert_eq!(report.status, MigrateStatus::NoSource);

        Ok(())
    }

    #[test]
    fn migrate_rerun_is_noop() -> Result<()> {
        let temp = tempdir()?;
        let root = temp.path();

        fs::create_dir_all(root.join(".metastack"))?;
        fs::write(root.join(".metastack").join("meta.json"), "{}")?;

        let report1 = migrate_state_root(root, ".metastack", ".intuition")?;
        assert_eq!(report1.status, MigrateStatus::Migrated);

        let report2 = migrate_state_root(root, ".metastack", ".intuition")?;
        assert_eq!(report2.status, MigrateStatus::AlreadyMigrated);

        Ok(())
    }

    #[test]
    fn migrate_rejects_same_source_and_destination() {
        let temp = tempdir().unwrap();
        let result = migrate_state_root(temp.path(), ".metastack", ".metastack");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("same"));
    }

    #[test]
    fn migrate_rejects_path_separators() {
        let temp = tempdir().unwrap();
        let result = migrate_state_root(temp.path(), ".metastack", "../escape");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path separators"));
    }

    #[test]
    fn migrate_rejects_dot_dot() {
        let temp = tempdir().unwrap();
        let result = migrate_state_root(temp.path(), "..", ".intuition");
        assert!(result.is_err());
    }
}

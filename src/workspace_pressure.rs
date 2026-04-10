use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use walkdir::WalkDir;

use crate::branding;
use crate::fs::{canonicalize_existing_dir, sibling_workspace_root};
use crate::listen::store::resolve_source_project_root;

#[cfg(unix)]
use std::ffi::CString;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

const KIB: u64 = 1024;
const MIB: u64 = KIB * 1024;
const GIB: u64 = MIB * 1024;
const DISK_WARNING_BYTES: u64 = 20 * GIB;
const DISK_CRITICAL_BYTES: u64 = 10 * GIB;
const MEMORY_WARNING_BYTES: u64 = 4 * GIB;
const MEMORY_CRITICAL_BYTES: u64 = 2 * GIB;
const WARNING_PERCENT: f64 = 0.15;
const CRITICAL_PERCENT: f64 = 0.08;
const TEST_MODE_ENV: &str = "METASTACK_TEST_MODE";
const TEST_FIXTURE_ENV: &str = "METASTACK_TEST_WORKSPACE_PRESSURE_FIXTURE";
const METASTACK_SOURCE_ROOT_ENV: &str = "METASTACK_SOURCE_ROOT";

#[cfg(unix)]
trait StatvfsWordExt {
    fn into_u64(self) -> u64;
}

#[cfg(unix)]
impl StatvfsWordExt for u64 {
    fn into_u64(self) -> u64 {
        self
    }
}

#[cfg(unix)]
impl StatvfsWordExt for u32 {
    fn into_u64(self) -> u64 {
        u64::from(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum WorkspacePressureLevel {
    Healthy,
    Warning,
    Critical,
}

impl WorkspacePressureLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResourceUsageSample {
    available_bytes: u64,
    total_bytes: u64,
}

impl ResourceUsageSample {
    fn available_percent(self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            self.available_bytes as f64 / self.total_bytes as f64
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WorkspacePressureSignal {
    Available {
        level: WorkspacePressureLevel,
        sample: ResourceUsageSample,
    },
    Unavailable {
        reason: String,
    },
}

impl WorkspacePressureSignal {
    fn disk(sample: Option<ResourceUsageSample>) -> Self {
        Self::from_sample(
            sample,
            DISK_WARNING_BYTES,
            DISK_CRITICAL_BYTES,
            "disk telemetry",
        )
    }

    fn memory(sample: Option<ResourceUsageSample>) -> Self {
        Self::from_sample(
            sample,
            MEMORY_WARNING_BYTES,
            MEMORY_CRITICAL_BYTES,
            "memory telemetry",
        )
    }

    fn from_sample(
        sample: Option<ResourceUsageSample>,
        warning_bytes: u64,
        critical_bytes: u64,
        unavailable_reason: &str,
    ) -> Self {
        let Some(sample) = sample.filter(|sample| sample.total_bytes > 0) else {
            return Self::Unavailable {
                reason: unavailable_reason.to_string(),
            };
        };

        let available_percent = sample.available_percent();
        let level = if sample.available_bytes <= critical_bytes
            && available_percent <= CRITICAL_PERCENT
        {
            WorkspacePressureLevel::Critical
        } else if sample.available_bytes <= warning_bytes && available_percent <= WARNING_PERCENT {
            WorkspacePressureLevel::Warning
        } else {
            WorkspacePressureLevel::Healthy
        };

        Self::Available { level, sample }
    }

    fn level(&self) -> Option<WorkspacePressureLevel> {
        match self {
            Self::Available { level, .. } => Some(*level),
            Self::Unavailable { .. } => None,
        }
    }

    fn render_line(&self, label: &str, availability_word: &str) -> String {
        match self {
            Self::Available { level, sample } => format!(
                "{label}: {} | {} {availability_word} of {} ({:.1}% {availability_word}).",
                level.label(),
                format_bytes(sample.available_bytes),
                format_bytes(sample.total_bytes),
                sample.available_percent() * 100.0,
            ),
            Self::Unavailable { reason } => {
                format!("{label}: telemetry unavailable ({reason}).")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspacePressureSummary {
    overall: Option<WorkspacePressureLevel>,
    disk: WorkspacePressureSignal,
    memory: WorkspacePressureSignal,
    managed_workspace_root: PathBuf,
    managed_workspace_footprint_bytes: u64,
    managed_workspace_count: usize,
}

impl WorkspacePressureSummary {
    /// Returns the normalized pressure label for shared listen and workspace surfaces.
    pub(crate) fn overall_label(&self) -> &'static str {
        self.overall
            .map(WorkspacePressureLevel::label)
            .unwrap_or("telemetry unavailable")
    }

    /// Returns the shared human-readable pressure summary lines for CLI and TUI surfaces.
    pub(crate) fn summary_lines(&self) -> Vec<String> {
        let clone_label = if self.managed_workspace_count == 1 {
            "clone"
        } else {
            "clones"
        };
        vec![
            format!("Workspace pressure: {}.", self.overall_label()),
            format!(
                "Managed workspace footprint: {} across {} managed {} under `{}`.",
                format_bytes(self.managed_workspace_footprint_bytes),
                self.managed_workspace_count,
                clone_label,
                self.managed_workspace_root.display()
            ),
            self.disk.render_line("Disk", "free"),
            self.memory.render_line("Memory", "available"),
            format!("Cleanup guidance: {}.", self.cleanup_guidance()),
        ]
    }

    /// Returns `true` when unattended listen startup must stop before claiming new work.
    pub(crate) fn should_block_unattended_startup(&self) -> bool {
        self.overall == Some(WorkspacePressureLevel::Critical)
    }

    /// Returns the shared critical-startup message for unattended listen runs.
    pub(crate) fn startup_block_message(&self) -> String {
        format!(
            "Critical workspace pressure blocks unattended `{}` startup before any claim or worker launch.\n{}",
            branding::COMMAND_NAME,
            self.summary_lines().join("\n")
        )
    }

    fn cleanup_guidance(&self) -> String {
        let prune_command = format!(
            "`{} workspace prune --dry-run --root .`",
            branding::COMMAND_NAME
        );
        let clean_targets_command = format!(
            "`{} workspace clean --target-only --root .`",
            branding::COMMAND_NAME
        );

        match self.overall {
            Some(WorkspacePressureLevel::Critical) => format!(
                "critical host pressure blocks unattended listen; reduce pressure before starting new work with {prune_command} and {clean_targets_command}"
            ),
            Some(WorkspacePressureLevel::Warning) => format!(
                "warning pressure detected; review safe removals with {prune_command} and reclaim build artifacts with {clean_targets_command}"
            ),
            Some(WorkspacePressureLevel::Healthy) if self.managed_workspace_count > 0 => format!(
                "guardrails are currently healthy; use {prune_command} or {clean_targets_command} when you want to reclaim managed workspace footprint"
            ),
            Some(WorkspacePressureLevel::Healthy) => format!(
                "no managed workspace cleanup is currently required; future cleanup uses {prune_command} and {clean_targets_command}"
            ),
            None => format!(
                "pressure telemetry is partial; use {prune_command} and {clean_targets_command} when reclaiming managed workspace footprint"
            ),
        }
    }
}

/// Assess disk and memory pressure plus the total managed-workspace footprint for the resolved
/// repository root.
///
/// Returns an error when the provided root cannot be resolved or when the managed workspace
/// footprint cannot be inspected.
pub(crate) fn assess_workspace_pressure(root: &Path) -> Result<WorkspacePressureSummary> {
    let source_root = resolve_pressure_source_root(root)?;
    let managed_workspace_root = sibling_workspace_root(&source_root)?;
    let managed_workspace_footprint_bytes = scan_directory_usage(&managed_workspace_root)?;
    let managed_workspace_count = count_managed_workspaces(&managed_workspace_root)?;

    if let Some(fixture) = test_fixture_summary(
        &managed_workspace_root,
        managed_workspace_footprint_bytes,
        managed_workspace_count,
    ) {
        return Ok(fixture);
    }

    Ok(build_workspace_pressure_summary(
        managed_workspace_root,
        managed_workspace_footprint_bytes,
        managed_workspace_count,
        probe_disk_usage_sample(root),
        probe_memory_usage_sample(),
    ))
}

fn build_workspace_pressure_summary(
    managed_workspace_root: PathBuf,
    managed_workspace_footprint_bytes: u64,
    managed_workspace_count: usize,
    disk_sample: Option<ResourceUsageSample>,
    memory_sample: Option<ResourceUsageSample>,
) -> WorkspacePressureSummary {
    let disk = WorkspacePressureSignal::disk(disk_sample);
    let memory = WorkspacePressureSignal::memory(memory_sample);
    let overall = [disk.level(), memory.level()].into_iter().flatten().max();

    WorkspacePressureSummary {
        overall,
        disk,
        memory,
        managed_workspace_root,
        managed_workspace_footprint_bytes,
        managed_workspace_count,
    }
}

fn resolve_pressure_source_root(root: &Path) -> Result<PathBuf> {
    if let Some(source_root) = env::var_os(METASTACK_SOURCE_ROOT_ENV) {
        let source_root = PathBuf::from(source_root);
        if source_root.is_dir() {
            return canonicalize_existing_dir(&source_root);
        }
    }

    let requested_root = canonicalize_existing_dir(root)?;
    resolve_source_project_root(&requested_root)
}

fn scan_directory_usage(root: &Path) -> Result<u64> {
    if !root.exists() {
        return Ok(0);
    }

    let du_output = Command::new("du").args(["-sk"]).arg(root).output();
    if let Ok(output) = du_output
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(kib) = stdout
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<u64>().ok())
        {
            return Ok(kib.saturating_mul(KIB));
        }
    }

    let mut total = 0u64;
    for entry in WalkDir::new(root) {
        let entry = entry.with_context(|| format!("failed to walk `{}`", root.display()))?;
        if entry.file_type().is_file() {
            total = total.saturating_add(
                entry
                    .metadata()
                    .with_context(|| format!("failed to inspect `{}`", entry.path().display()))?
                    .len(),
            );
        }
    }
    Ok(total)
}

fn count_managed_workspaces(workspace_root: &Path) -> Result<usize> {
    if !workspace_root.exists() {
        return Ok(0);
    }

    let mut count = 0usize;
    let entries = fs::read_dir(workspace_root)
        .with_context(|| format!("failed to read `{}`", workspace_root.display()))?;
    for entry in entries {
        let entry =
            entry.with_context(|| format!("failed to read `{}`", workspace_root.display()))?;
        if !entry
            .file_type()
            .with_context(|| format!("failed to inspect `{}`", entry.path().display()))?
            .is_dir()
        {
            continue;
        }

        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };

        if looks_like_ticket_identifier(name) || name.starts_with("improve-") {
            if path.join(".git").exists() {
                count += 1;
            }
            continue;
        }

        if name != "review-runs" {
            continue;
        }

        let review_entries =
            fs::read_dir(&path).with_context(|| format!("failed to read `{}`", path.display()))?;
        for review_entry in review_entries {
            let review_entry =
                review_entry.with_context(|| format!("failed to read `{}`", path.display()))?;
            if !review_entry
                .file_type()
                .with_context(|| format!("failed to inspect `{}`", review_entry.path().display()))?
                .is_dir()
            {
                continue;
            }

            let review_path = review_entry.path();
            let Some(review_name) = review_path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if review_name.starts_with("pr-") && review_path.join(".git").exists() {
                count += 1;
            }
        }
    }

    Ok(count)
}

fn looks_like_ticket_identifier(value: &str) -> bool {
    let Some((team, number)) = value.split_once('-') else {
        return false;
    };
    !team.is_empty()
        && !number.is_empty()
        && team
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
        && number.chars().all(|character| character.is_ascii_digit())
}

fn format_bytes(bytes: u64) -> String {
    let value = bytes as f64;
    if value >= GIB as f64 {
        format!("{:.2} GiB", value / GIB as f64)
    } else if value >= MIB as f64 {
        format!("{:.2} MiB", value / MIB as f64)
    } else if value >= KIB as f64 {
        format!("{:.2} KiB", value / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn test_fixture_summary(
    managed_workspace_root: &Path,
    managed_workspace_footprint_bytes: u64,
    managed_workspace_count: usize,
) -> Option<WorkspacePressureSummary> {
    if env::var(TEST_MODE_ENV).ok().as_deref() != Some("1") {
        return None;
    }

    let fixture = env::var(TEST_FIXTURE_ENV).ok()?;
    let healthy_disk = Some(ResourceUsageSample {
        available_bytes: 120 * GIB,
        total_bytes: 512 * GIB,
    });
    let healthy_memory = Some(ResourceUsageSample {
        available_bytes: 24 * GIB,
        total_bytes: 64 * GIB,
    });
    let (disk, memory) = match fixture.as_str() {
        "healthy" => (healthy_disk, healthy_memory),
        "warning-disk" => (
            Some(ResourceUsageSample {
                available_bytes: 18 * GIB,
                total_bytes: 120 * GIB,
            }),
            healthy_memory,
        ),
        "critical-disk" => (
            Some(ResourceUsageSample {
                available_bytes: 8 * GIB,
                total_bytes: 120 * GIB,
            }),
            healthy_memory,
        ),
        "warning-memory" => (
            healthy_disk,
            Some(ResourceUsageSample {
                available_bytes: 3 * GIB,
                total_bytes: 24 * GIB,
            }),
        ),
        "critical-memory" => (
            healthy_disk,
            Some(ResourceUsageSample {
                available_bytes: GIB,
                total_bytes: 24 * GIB,
            }),
        ),
        "unavailable" => (None, None),
        _ => return None,
    };

    Some(build_workspace_pressure_summary(
        managed_workspace_root.to_path_buf(),
        managed_workspace_footprint_bytes,
        managed_workspace_count,
        disk,
        memory,
    ))
}

fn probe_disk_usage_sample(root: &Path) -> Option<ResourceUsageSample> {
    let source_root = resolve_pressure_source_root(root).ok()?;
    let managed_workspace_root = sibling_workspace_root(&source_root).ok()?;
    let probe_path = nearest_existing_ancestor(&managed_workspace_root)
        .or_else(|| nearest_existing_ancestor(root))?;

    #[cfg(unix)]
    {
        let c_path = CString::new(probe_path.as_os_str().as_bytes()).ok()?;
        let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        let result = unsafe { libc::statvfs(c_path.as_ptr(), stats.as_mut_ptr()) };
        if result != 0 {
            return None;
        }
        let stats = unsafe { stats.assume_init() };
        let block_size = if stats.f_frsize > 0 {
            stats.f_frsize
        } else {
            stats.f_bsize
        }
        .into_u64();
        let total_blocks = stats.f_blocks.into_u64();
        let available_blocks = stats.f_bavail.into_u64();
        return Some(ResourceUsageSample {
            available_bytes: available_blocks.saturating_mul(block_size),
            total_bytes: total_blocks.saturating_mul(block_size),
        });
    }

    #[allow(unreachable_code)]
    None
}

fn nearest_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut candidate = Some(path);
    while let Some(current) = candidate {
        if current.exists() {
            return Some(current.to_path_buf());
        }
        candidate = current.parent();
    }
    None
}

fn probe_memory_usage_sample() -> Option<ResourceUsageSample> {
    #[cfg(target_os = "linux")]
    {
        return probe_linux_memory_usage_sample();
    }

    #[cfg(target_os = "macos")]
    {
        return probe_macos_memory_usage_sample();
    }

    #[allow(unreachable_code)]
    None
}

#[cfg(target_os = "linux")]
fn probe_linux_memory_usage_sample() -> Option<ResourceUsageSample> {
    let contents = fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kib = None;
    let mut available_kib = None;

    for line in contents.lines() {
        if let Some(value) = parse_meminfo_line(line, "MemTotal:") {
            total_kib = Some(value);
        } else if let Some(value) = parse_meminfo_line(line, "MemAvailable:") {
            available_kib = Some(value);
        }
    }

    Some(ResourceUsageSample {
        available_bytes: available_kib?.saturating_mul(KIB),
        total_bytes: total_kib?.saturating_mul(KIB),
    })
}

#[cfg(target_os = "linux")]
fn parse_meminfo_line(line: &str, prefix: &str) -> Option<u64> {
    line.strip_prefix(prefix)?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()
}

#[cfg(target_os = "macos")]
fn probe_macos_memory_usage_sample() -> Option<ResourceUsageSample> {
    let total_output = Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()?;
    if !total_output.status.success() {
        return None;
    }
    let total_bytes = String::from_utf8_lossy(&total_output.stdout)
        .trim()
        .parse::<u64>()
        .ok()?;

    let vm_stat_output = Command::new("vm_stat").output().ok()?;
    if !vm_stat_output.status.success() {
        return None;
    }
    let vm_stat = String::from_utf8_lossy(&vm_stat_output.stdout);
    let page_size = vm_stat
        .lines()
        .next()
        .and_then(extract_first_integer)
        .unwrap_or(4096);
    let mut available_pages = 0u64;

    for (prefix, include) in [
        ("Pages free:", true),
        ("Pages inactive:", true),
        ("Pages speculative:", true),
        ("Pages purgeable:", true),
    ] {
        if !include {
            continue;
        }
        if let Some(value) = vm_stat
            .lines()
            .find(|line| line.trim_start().starts_with(prefix))
            .and_then(extract_first_integer)
        {
            available_pages = available_pages.saturating_add(value);
        }
    }

    Some(ResourceUsageSample {
        available_bytes: available_pages.saturating_mul(page_size),
        total_bytes,
    })
}

#[cfg(target_os = "macos")]
fn extract_first_integer(line: &str) -> Option<u64> {
    let digits = line
        .chars()
        .map(|character| {
            if character.is_ascii_digit() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>();
    digits
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    use super::{
        GIB, ResourceUsageSample, WorkspacePressureLevel, WorkspacePressureSummary,
        build_workspace_pressure_summary,
    };
    use std::path::PathBuf;

    fn summary(
        disk: Option<ResourceUsageSample>,
        memory: Option<ResourceUsageSample>,
    ) -> WorkspacePressureSummary {
        build_workspace_pressure_summary(
            PathBuf::from("/tmp/workspaces"),
            12 * GIB,
            3,
            disk,
            memory,
        )
    }

    #[test]
    fn workspace_pressure_reports_healthy_when_signals_have_headroom() {
        let summary = summary(
            Some(ResourceUsageSample {
                available_bytes: 120 * GIB,
                total_bytes: 512 * GIB,
            }),
            Some(ResourceUsageSample {
                available_bytes: 24 * GIB,
                total_bytes: 64 * GIB,
            }),
        );

        assert_eq!(summary.overall_label(), "healthy");
        assert!(!summary.should_block_unattended_startup());
    }

    #[test]
    fn workspace_pressure_reports_warning_for_low_disk() {
        let summary = summary(
            Some(ResourceUsageSample {
                available_bytes: 18 * GIB,
                total_bytes: 120 * GIB,
            }),
            Some(ResourceUsageSample {
                available_bytes: 24 * GIB,
                total_bytes: 64 * GIB,
            }),
        );

        assert_eq!(summary.overall, Some(WorkspacePressureLevel::Warning));
        assert!(summary.summary_lines()[2].contains("warning"));
    }

    #[test]
    fn workspace_pressure_reports_critical_for_low_memory() {
        let summary = summary(
            Some(ResourceUsageSample {
                available_bytes: 120 * GIB,
                total_bytes: 512 * GIB,
            }),
            Some(ResourceUsageSample {
                available_bytes: GIB,
                total_bytes: 24 * GIB,
            }),
        );

        assert_eq!(summary.overall, Some(WorkspacePressureLevel::Critical));
        assert!(summary.should_block_unattended_startup());
        assert!(
            summary
                .startup_block_message()
                .contains("Critical workspace pressure")
        );
    }

    #[test]
    fn workspace_pressure_keeps_large_disk_percent_only_case_healthy() {
        let summary = summary(
            Some(ResourceUsageSample {
                available_bytes: 43 * GIB,
                total_bytes: 926 * GIB,
            }),
            Some(ResourceUsageSample {
                available_bytes: 24 * GIB,
                total_bytes: 64 * GIB,
            }),
        );

        assert_eq!(summary.overall, Some(WorkspacePressureLevel::Healthy));
        assert!(!summary.should_block_unattended_startup());
    }

    #[test]
    fn workspace_pressure_keeps_memory_bytes_only_case_healthy() {
        let summary = summary(
            Some(ResourceUsageSample {
                available_bytes: 120 * GIB,
                total_bytes: 512 * GIB,
            }),
            Some(ResourceUsageSample {
                available_bytes: 3 * GIB,
                total_bytes: 16 * GIB,
            }),
        );

        assert_eq!(summary.overall, Some(WorkspacePressureLevel::Healthy));
        assert!(!summary.should_block_unattended_startup());
    }

    #[test]
    fn workspace_pressure_reports_telemetry_unavailable_when_signals_are_missing() {
        let summary = summary(None, None);

        assert_eq!(summary.overall, None);
        assert_eq!(summary.overall_label(), "telemetry unavailable");
        assert!(summary.summary_lines()[2].contains("telemetry unavailable"));
        assert!(summary.summary_lines()[3].contains("telemetry unavailable"));
    }
}

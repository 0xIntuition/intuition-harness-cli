use std::ffi::OsStr;
use std::fs;
use std::io::{self, IsTerminal, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Local};
use serde::Deserialize;
use walkdir::WalkDir;

use crate::cli::{WorkspaceCleanArgs, WorkspaceListArgs, WorkspacePruneArgs};
use crate::fs::{canonicalize_existing_dir, ensure_workspace_path_is_safe, sibling_workspace_root};
use crate::linear::{IssueListFilters, load_linear_command_context};
use crate::listen::store::{ListenProjectStore, resolve_source_project_root};

#[derive(Debug, Clone)]
struct WorkspaceContext {
    source_root: PathBuf,
    workspace_root: PathBuf,
}

#[derive(Debug, Clone)]
struct WorkspaceEntry {
    ticket: String,
    path: PathBuf,
    branch: String,
    disk_usage_bytes: u64,
    last_modified: SystemTime,
    git: WorkspaceGitSignals,
}

#[derive(Debug, Clone, Default)]
struct WorkspaceGitSignals {
    has_uncommitted_changes: bool,
    has_unpushed_commits: bool,
    is_detached: bool,
}

#[derive(Debug, Clone)]
struct WorkspaceListRecord {
    entry: WorkspaceEntry,
    linear_state: String,
    linear_is_removal_candidate: bool,
    pr_status: PullRequestStatus,
}

#[derive(Debug, Clone, Default)]
enum PullRequestStatus {
    Open,
    Merged,
    Closed,
    #[default]
    Unavailable,
    None,
}

#[derive(Debug, Clone)]
enum GithubPrLookup {
    Available(Vec<GithubPullRequest>),
    Unavailable(String),
}

#[derive(Debug, Clone, Deserialize)]
struct GithubPullRequest {
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    state: String,
}

#[derive(Debug, Clone)]
struct CleanOutcome {
    target_dirs_removed: usize,
    bytes_reclaimed: u64,
    lines: Vec<String>,
}

#[derive(Debug, Clone)]
struct PruneDecision {
    record: WorkspaceListRecord,
    action: PruneAction,
    reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PruneAction {
    Remove,
    Keep,
}

/// Lists discovered listener workspace clones, enriching each entry with Linear state and optional
/// GitHub pull-request status. Returns an error when repository or Linear metadata cannot be
/// resolved.
pub(crate) async fn run_workspace_list(args: &WorkspaceListArgs) -> Result<String> {
    let context = resolve_workspace_context(&args.client.root)?;
    let entries = discover_workspace_entries(&context)?;
    if entries.is_empty() {
        return Ok(format!(
            "No workspace clones found under `{}`.",
            context.workspace_root.display()
        ));
    }

    let linear = load_linear_command_context(&args.client, None)?;
    let github = discover_github_prs(&context.source_root);
    let records = enrich_workspace_entries(entries, &linear.service, &github).await?;

    let mut lines = vec![
        format!("Workspace root: {}", context.workspace_root.display()),
        "TICKET  BRANCH  SIZE  MODIFIED  GIT  LINEAR  PR  SAFE".to_string(),
    ];
    for record in &records {
        let safe = if record.linear_is_removal_candidate {
            "candidate"
        } else {
            "-"
        };
        lines.push(format!(
            "{}  {}  {}  {}  {}  {}  {}  {}",
            record.entry.ticket,
            record.entry.branch,
            format_bytes(record.entry.disk_usage_bytes),
            format_system_time(record.entry.last_modified),
            record.entry.git.display_label(),
            record.linear_state,
            record.pr_status.display_label(),
            safe,
        ));
    }

    if let GithubPrLookup::Unavailable(reason) = github {
        lines.push(String::new());
        lines.push(format!("GitHub PR data unavailable: {reason}"));
    }

    Ok(lines.join("\n"))
}

/// Removes one workspace clone or the `target/` directories within matching clones. Returns an
/// error when the requested ticket clone cannot be found or when a destructive path falls outside
/// the resolved sibling workspace root.
pub(crate) fn run_workspace_clean(args: &WorkspaceCleanArgs) -> Result<String> {
    let context = resolve_workspace_context(&args.root.root)?;
    if args.target_only {
        return clean_targets(&context, args.ticket.as_deref());
    }

    let ticket = args
        .ticket
        .as_deref()
        .ok_or_else(|| anyhow!("workspace ticket is required unless `--target-only` is used"))?;
    let entry = find_workspace_entry(&context, ticket)?;
    let mut lines = render_clean_safety_lines(&entry);
    if !args.force {
        confirm_workspace_removal(ticket, &entry.path)?;
    }

    let reclaimed = remove_workspace_clone(&context, &entry)?;
    lines.push(format!(
        "Removed workspace `{ticket}` and freed {}.",
        format_bytes(reclaimed)
    ));
    Ok(lines.join("\n"))
}

/// Removes completed workspace clones when their Linear ticket is done or cancelled, preserving
/// clones with open pull requests or local safety risks. Returns an error when repository or
/// Linear metadata cannot be resolved.
pub(crate) async fn run_workspace_prune(args: &WorkspacePruneArgs) -> Result<String> {
    let context = resolve_workspace_context(&args.client.root)?;
    let entries = discover_workspace_entries(&context)?;
    if entries.is_empty() {
        return Ok(format!(
            "Removed 0 clones, freed {}. Kept 0 clones.\nWorkspace root: {}",
            format_bytes(0),
            context.workspace_root.display()
        ));
    }

    let linear = load_linear_command_context(&args.client, None)?;
    let github = discover_github_prs(&context.source_root);
    let records = enrich_workspace_entries(entries, &linear.service, &github).await?;
    let decisions = records
        .into_iter()
        .map(build_prune_decision)
        .collect::<Vec<_>>();

    let mut removed = 0usize;
    let mut kept = 0usize;
    let mut freed_bytes = 0u64;
    let mut lines = vec![format!(
        "{} workspace prune preview:",
        if args.dry_run { "Dry-run" } else { "Active" }
    )];

    for decision in &decisions {
        let action = match decision.action {
            PruneAction::Remove => "REMOVE",
            PruneAction::Keep => "KEEP",
        };
        lines.push(format!(
            "{}  {}  {}  {}  {}",
            action,
            decision.record.entry.ticket,
            format_bytes(decision.record.entry.disk_usage_bytes),
            decision.record.linear_state,
            decision.reason,
        ));
    }

    if let GithubPrLookup::Unavailable(reason) = github {
        lines.push(String::new());
        lines.push(format!(
            "GitHub PR data unavailable; using Linear completion state only: {reason}"
        ));
    }

    if !args.dry_run {
        let removals = decisions
            .iter()
            .filter(|d| d.action == PruneAction::Remove)
            .count();
        if removals > 0 && !args.force {
            let prompt = format!("Remove {removals} workspace clone{}? [y/N]: ", if removals == 1 { "" } else { "s" });
            if io::stdin().is_terminal() {
                print!("{prompt}");
                io::stdout().flush().context("failed to flush confirmation prompt")?;
            } else {
                eprint!("{prompt}");
                io::stderr().flush().context("failed to flush confirmation prompt")?;
            }
            let mut input = String::new();
            io::stdin().read_line(&mut input).context("failed to read confirmation input")?;
            if !matches!(input.trim(), "y" | "Y" | "yes" | "YES") {
                bail!("workspace prune canceled");
            }
        }

        for decision in &decisions {
            match decision.action {
                PruneAction::Remove => {
                    freed_bytes += remove_workspace_clone(&context, &decision.record.entry)?;
                    removed += 1;
                }
                PruneAction::Keep => kept += 1,
            }
        }
    } else {
        for decision in &decisions {
            match decision.action {
                PruneAction::Remove => {
                    freed_bytes += decision.record.entry.disk_usage_bytes;
                    removed += 1;
                }
                PruneAction::Keep => kept += 1,
            }
        }
    }

    lines.push(String::new());
    lines.push(format!(
        "Removed {removed} clones, freed {}. Kept {kept} clones.",
        format_bytes(freed_bytes)
    ));
    Ok(lines.join("\n"))
}

fn resolve_workspace_context(root: &Path) -> Result<WorkspaceContext> {
    let source_root = resolve_source_project_root(&canonicalize_existing_dir(root)?)?;
    let workspace_root = sibling_workspace_root(&source_root)?;
    Ok(WorkspaceContext {
        source_root,
        workspace_root,
    })
}

fn discover_workspace_entries(context: &WorkspaceContext) -> Result<Vec<WorkspaceEntry>> {
    let entries = match fs::read_dir(&context.workspace_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read `{}`", context.workspace_root.display()));
        }
    };

    let mut discovered = Vec::new();
    for entry in entries {
        let entry = entry
            .with_context(|| format!("failed to read `{}`", context.workspace_root.display()))?;
        if !entry
            .file_type()
            .with_context(|| format!("failed to inspect `{}`", entry.path().display()))?
            .is_dir()
        {
            continue;
        }

        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !looks_like_ticket_identifier(name) || !entry.path().join(".git").exists() {
            continue;
        }

        discovered.push(read_workspace_entry(context, name, &entry.path())?);
    }

    discovered.sort_by(|left, right| left.ticket.cmp(&right.ticket));
    Ok(discovered)
}

fn find_workspace_entry(context: &WorkspaceContext, ticket: &str) -> Result<WorkspaceEntry> {
    discover_workspace_entries(context)?
        .into_iter()
        .find(|entry| entry.ticket.eq_ignore_ascii_case(ticket))
        .ok_or_else(|| {
            anyhow!(
                "workspace clone `{ticket}` was not found under `{}`",
                context.workspace_root.display()
            )
        })
}

fn read_workspace_entry(
    context: &WorkspaceContext,
    ticket: &str,
    workspace_path: &Path,
) -> Result<WorkspaceEntry> {
    ensure_workspace_path_is_safe(
        &context.source_root,
        &context.workspace_root,
        workspace_path,
    )?;
    let branch = git_stdout(workspace_path, &["rev-parse", "--abbrev-ref", "HEAD"])
        .context("failed to inspect the workspace branch")?;
    let status = git_stdout(workspace_path, &["status", "--porcelain"])
        .context("failed to inspect local workspace changes")?;
    let has_uncommitted_changes = !status.trim().is_empty();
    let is_detached = branch == "HEAD";
    let has_unpushed_commits = workspace_has_unpushed_commits(workspace_path)?;
    let (disk_usage_bytes, last_modified) = scan_workspace_usage(workspace_path)?;

    Ok(WorkspaceEntry {
        ticket: ticket.to_string(),
        path: workspace_path.to_path_buf(),
        branch,
        disk_usage_bytes,
        last_modified,
        git: WorkspaceGitSignals {
            has_uncommitted_changes,
            has_unpushed_commits,
            is_detached,
        },
    })
}

async fn enrich_workspace_entries<C>(
    entries: Vec<WorkspaceEntry>,
    linear: &crate::linear::LinearService<C>,
    github: &GithubPrLookup,
) -> Result<Vec<WorkspaceListRecord>>
where
    C: crate::linear::LinearClient,
{
    let mut records = Vec::with_capacity(entries.len());
    for entry in entries {
        let (linear_state, linear_is_removal_candidate) = match linear
            .find_issue_by_identifier(
                &entry.ticket,
                IssueListFilters {
                    team: issue_team_key(&entry.ticket),
                    limit: 250,
                    ..IssueListFilters::default()
                },
            )
            .await?
        {
            Some(issue) => issue
                .state
                .as_ref()
                .map(|state| {
                    (
                        state.name.clone(),
                        linear_state_is_removal_candidate(&state.name, state.kind.as_deref()),
                    )
                })
                .unwrap_or_else(|| ("Unknown".to_string(), false)),
            None => ("Missing".to_string(), false),
        };
        let pr_status = match github {
            GithubPrLookup::Available(prs) => prs
                .iter()
                .find(|pr| pr.head_ref_name == entry.branch)
                .map(|pr| PullRequestStatus::from_gh_state(&pr.state))
                .unwrap_or(PullRequestStatus::None),
            GithubPrLookup::Unavailable(_) => PullRequestStatus::Unavailable,
        };
        records.push(WorkspaceListRecord {
            entry,
            linear_state,
            linear_is_removal_candidate,
            pr_status,
        });
    }

    Ok(records)
}

fn discover_github_prs(root: &Path) -> GithubPrLookup {
    let output = Command::new("gh")
        .args([
            "pr",
            "list",
            "--state",
            "all",
            "--limit",
            "200",
            "--json",
            "headRefName,state",
        ])
        .current_dir(root)
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) => return GithubPrLookup::Unavailable(error.to_string()),
    };
    if !output.status.success() {
        return GithubPrLookup::Unavailable(String::from_utf8_lossy(&output.stderr).trim().into());
    }

    match serde_json::from_slice::<Vec<GithubPullRequest>>(&output.stdout) {
        Ok(prs) => GithubPrLookup::Available(prs),
        Err(error) => GithubPrLookup::Unavailable(error.to_string()),
    }
}

fn clean_targets(context: &WorkspaceContext, ticket: Option<&str>) -> Result<String> {
    let entries = discover_workspace_entries(context)?;
    let selected = match ticket {
        Some(ticket) => vec![find_workspace_entry(context, ticket)?],
        None => entries,
    };

    let mut outcome = CleanOutcome {
        target_dirs_removed: 0,
        bytes_reclaimed: 0,
        lines: Vec::new(),
    };
    let selected_count = selected.len();
    for entry in selected {
        let targets = find_target_dirs(context, &entry)?;
        let target_count = targets.len();
        if target_count == 0 {
            outcome
                .lines
                .push(format!("{}: no target/ directories found.", entry.ticket));
            continue;
        }

        let mut reclaimed = 0u64;
        for target in targets {
            reclaimed += scan_workspace_usage(&target)?.0;
            fs::remove_dir_all(&target)
                .with_context(|| format!("failed to remove `{}`", target.display()))?;
            outcome.target_dirs_removed += 1;
        }

        outcome.bytes_reclaimed += reclaimed;
        outcome.lines.push(format!(
            "{}: removed {} target director{} and freed {}.",
            entry.ticket,
            target_count,
            if target_count == 1 { "y" } else { "ies" },
            format_bytes(reclaimed),
        ));
    }

    outcome.lines.push(format!(
        "Removed {} target director{} across {} workspace clone{} and freed {}.",
        outcome.target_dirs_removed,
        if outcome.target_dirs_removed == 1 {
            "y"
        } else {
            "ies"
        },
        selected_count,
        if selected_count == 1 { "" } else { "s" },
        format_bytes(outcome.bytes_reclaimed),
    ));
    Ok(outcome.lines.join("\n"))
}

fn find_target_dirs(context: &WorkspaceContext, entry: &WorkspaceEntry) -> Result<Vec<PathBuf>> {
    ensure_workspace_path_is_safe(&context.source_root, &context.workspace_root, &entry.path)?;
    // Check top-level target/ first (where Cargo puts build artifacts).
    // This avoids walking the entire clone tree which can be very slow for large workspaces.
    let top_level_target = entry.path.join("target");
    if top_level_target.is_dir() {
        let canonical = top_level_target
            .canonicalize()
            .with_context(|| format!("failed to resolve `{}`", top_level_target.display()))?;
        if canonical.starts_with(&entry.path) {
            return Ok(vec![top_level_target]);
        }
    }

    // Fallback: walk the tree for nested target/ dirs (e.g., workspace members).
    let mut targets = Vec::new();
    for node in WalkDir::new(&entry.path).max_depth(3) {
        let node = node.with_context(|| format!("failed to walk `{}`", entry.path.display()))?;
        if !node.file_type().is_dir() || node.file_name() != OsStr::new("target") {
            continue;
        }

        let path = node.path().to_path_buf();
        let canonical = path
            .canonicalize()
            .with_context(|| format!("failed to resolve `{}`", path.display()))?;
        if !canonical.starts_with(&entry.path) {
            bail!(
                "refusing to remove target directory outside workspace `{}`",
                canonical.display()
            );
        }
        targets.push(path);
    }

    Ok(targets)
}

fn render_clean_safety_lines(entry: &WorkspaceEntry) -> Vec<String> {
    let mut lines = vec![format!(
        "Workspace `{}` safety: {}",
        entry.ticket,
        entry.git.display_label()
    )];
    if entry.git.has_uncommitted_changes {
        lines.push("Warning: uncommitted changes will be deleted.".to_string());
    }
    if entry.git.has_unpushed_commits {
        lines.push("Warning: unpushed commits were detected.".to_string());
    }
    if entry.git.is_detached {
        lines.push("Warning: workspace HEAD is detached.".to_string());
    }
    lines
}

fn confirm_workspace_removal(ticket: &str, path: &Path) -> Result<()> {
    let prompt = format!(
        "Delete workspace `{ticket}` at `{}`? [y/N]: ",
        path.display()
    );
    if io::stdin().is_terminal() {
        print!("{prompt}");
        io::stdout()
            .flush()
            .context("failed to flush confirmation prompt")?;
    } else {
        eprint!("{prompt}");
        io::stderr()
            .flush()
            .context("failed to flush confirmation prompt")?;
    }

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read confirmation input")?;
    if !matches!(input.trim(), "y" | "Y" | "yes" | "YES") {
        bail!("workspace removal canceled");
    }
    Ok(())
}

fn remove_workspace_clone(context: &WorkspaceContext, entry: &WorkspaceEntry) -> Result<u64> {
    ensure_workspace_path_is_safe(&context.source_root, &context.workspace_root, &entry.path)?;
    let reclaimed = entry.disk_usage_bytes;
    fs::remove_dir_all(&entry.path)
        .with_context(|| format!("failed to remove `{}`", entry.path.display()))?;
    let store = ListenProjectStore::resolve(&context.source_root)?;
    store.remove_ticket_artifacts(&entry.ticket)?;
    Ok(reclaimed)
}

fn build_prune_decision(record: WorkspaceListRecord) -> PruneDecision {
    if !record.linear_is_removal_candidate {
        return PruneDecision {
            record,
            action: PruneAction::Keep,
            reason: "ticket is not Done or Cancelled".to_string(),
        };
    }
    if matches!(record.pr_status, PullRequestStatus::Open) {
        return PruneDecision {
            record,
            action: PruneAction::Keep,
            reason: "branch pull request is still open".to_string(),
        };
    }
    if record.entry.git.has_unpushed_commits {
        return PruneDecision {
            record,
            action: PruneAction::Keep,
            reason: "unpushed commits detected".to_string(),
        };
    }
    if record.entry.git.has_uncommitted_changes {
        return PruneDecision {
            record,
            action: PruneAction::Keep,
            reason: "uncommitted changes detected".to_string(),
        };
    }

    let reason = match record.pr_status {
        PullRequestStatus::Merged => "ticket completed and PR is merged",
        PullRequestStatus::Closed => "ticket completed and PR is closed",
        PullRequestStatus::Unavailable => "ticket completed; PR data unavailable",
        PullRequestStatus::None => "ticket completed and no PR was found",
        PullRequestStatus::Open => unreachable!("open PRs are handled earlier"),
    };
    PruneDecision {
        record,
        action: PruneAction::Remove,
        reason: reason.to_string(),
    }
}

fn workspace_has_unpushed_commits(workspace_path: &Path) -> Result<bool> {
    let upstream = git_stdout(
        workspace_path,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    );
    let count = match upstream {
        Ok(_) => git_stdout(
            workspace_path,
            &["rev-list", "--count", "@{upstream}..HEAD"],
        ),
        Err(_) => git_stdout(
            workspace_path,
            &["rev-list", "--count", "origin/main..HEAD"],
        ),
    }?;
    let count = count
        .trim()
        .parse::<u64>()
        .context("failed to parse git ahead count")?;
    Ok(count > 0)
}

fn scan_workspace_usage(root: &Path) -> Result<(u64, SystemTime)> {
    let last_modified = fs::metadata(root)
        .with_context(|| format!("failed to inspect `{}`", root.display()))?
        .modified()
        .with_context(|| format!("failed to read modified time for `{}`", root.display()))?;

    // Use `du -sk` for fast disk usage instead of walking every file.
    let bytes = match Command::new("du")
        .args(["-sk"])
        .arg(root)
        .output()
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout
                .split_whitespace()
                .next()
                .and_then(|kb| kb.parse::<u64>().ok())
                .unwrap_or(0)
                * 1024 // du -sk reports in kilobytes
        }
        _ => 0,
    };

    Ok((bytes, last_modified))
}

fn looks_like_ticket_identifier(value: &str) -> bool {
    let Some((team, number)) = value.split_once('-') else {
        return false;
    };
    !team.is_empty()
        && !number.is_empty()
        && team.chars().all(|ch| ch.is_ascii_alphanumeric())
        && number.chars().all(|ch| ch.is_ascii_digit())
}

fn issue_team_key(identifier: &str) -> Option<String> {
    identifier
        .split_once('-')
        .map(|(team, _)| team.to_string())
        .filter(|team| !team.is_empty())
}

fn linear_state_is_removal_candidate(state_name: &str, state_kind: Option<&str>) -> bool {
    if matches!(
        state_kind.map(|kind| kind.trim().to_ascii_lowercase()),
        Some(kind) if matches!(kind.as_str(), "completed" | "canceled")
    ) {
        return true;
    }

    matches!(
        state_name.trim().to_ascii_lowercase().as_str(),
        "done" | "cancelled" | "canceled"
    )
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let value = bytes as f64;
    if value >= GIB {
        format!("{:.2} GiB", value / GIB)
    } else if value >= MIB {
        format!("{:.2} MiB", value / MIB)
    } else if value >= KIB {
        format!("{:.2} KiB", value / KIB)
    } else {
        format!("{bytes} B")
    }
}

fn format_system_time(value: SystemTime) -> String {
    let value: DateTime<Local> = value.into();
    value.format("%Y-%m-%d %H:%M").to_string()
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

impl WorkspaceGitSignals {
    fn display_label(&self) -> String {
        let mut labels = Vec::new();
        if self.has_uncommitted_changes {
            labels.push("dirty");
        }
        if self.has_unpushed_commits {
            labels.push("ahead");
        }
        if self.is_detached {
            labels.push("detached");
        }
        if labels.is_empty() {
            "clean".to_string()
        } else {
            labels.join("+")
        }
    }
}

impl PullRequestStatus {
    fn from_gh_state(state: &str) -> Self {
        match state.trim().to_ascii_uppercase().as_str() {
            "OPEN" => Self::Open,
            "MERGED" => Self::Merged,
            "CLOSED" => Self::Closed,
            _ => Self::Unavailable,
        }
    }

    fn display_label(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Merged => "merged",
            Self::Closed => "closed",
            Self::Unavailable => "unavailable",
            Self::None => "none",
        }
    }
}

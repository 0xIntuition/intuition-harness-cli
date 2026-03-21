use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use time::OffsetDateTime;
use time::macros::format_description;
use walkdir::{DirEntry, WalkDir};

use crate::agents::run_agent_capture;
use crate::cli::{RoadmapArgs, RunAgentArgs};
use crate::config::{AGENT_ROUTE_BACKLOG_ROADMAP, AppConfig, load_required_planning_meta};
use crate::fs::{PlanningPaths, canonicalize_existing_dir, display_path, ensure_dir, write_text_file};
use crate::linear::{
    IssueListFilters, LinearClient, LinearService, ProjectSummary, ProjectUpdateRequest,
};
use crate::load_linear_command_context;
use crate::text_diff::render_text_diff;

const ROADMAP_FILE_NAME: &str = "roadmap.md";
const ROADMAP_MARKER_START: &str = "<!-- metastack:roadmap:start -->";
const ROADMAP_MARKER_END: &str = "<!-- metastack:roadmap:end -->";
const ROADMAP_MAX_SOURCE_BYTES: usize = 64 * 1024;
const NON_INTERACTIVE_MAX_FOLLOW_UP_QUESTIONS: usize = 3;

#[derive(Debug, Clone, Serialize)]
struct SourceManifestEntry {
    label: String,
    origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes: Option<usize>,
}

#[derive(Debug, Clone)]
struct CollectedSource {
    label: String,
    body: String,
}

#[derive(Debug, Clone, Serialize)]
struct RoadmapRunSummary {
    run_id: String,
    route_key: String,
    roadmap_path: String,
    proposal_path: String,
    source_manifest_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff_path: Option<String>,
    apply_requested: bool,
    repo_write_status: String,
    pre_sync_divergence: String,
    post_sync_divergence: String,
    project_id: String,
    project_name: String,
    sources_used: usize,
    sources_skipped: usize,
    sources_errored: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    apply_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoadmapWriteStatus {
    NotApplied,
    Created,
    Updated,
    Unchanged,
}

impl RoadmapWriteStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::NotApplied => "not-applied",
            Self::Created => "created",
            Self::Updated => "updated",
            Self::Unchanged => "unchanged",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DivergenceStatus {
    InSync,
    RepoAhead,
    LinearAhead,
}

impl DivergenceStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::InSync => "in-sync",
            Self::RepoAhead => "repo-ahead",
            Self::LinearAhead => "linear-ahead",
        }
    }
}

/// Drafts or refreshes the canonical repo-root roadmap and optionally syncs it to Linear.
///
/// Returns an error when repo setup is missing, the configured Linear project cannot be resolved,
/// required non-interactive inputs are absent, source discovery fails, agent generation fails, or
/// a requested apply cannot complete safely.
pub async fn run_roadmap(args: &RoadmapArgs) -> Result<()> {
    let root = canonicalize_existing_dir(&args.client.root)?;
    let _app_config = AppConfig::load()?;
    let planning_meta = load_required_planning_meta(&root, "backlog roadmap")?;
    let linear_context = load_linear_command_context(&args.client, None)?;
    let service = linear_context.service;
    let default_project_id = linear_context.default_project_id;
    let project_id = planning_meta
        .linear
        .project_id
        .clone()
        .or(default_project_id)
        .ok_or_else(|| anyhow!("`meta backlog roadmap` requires a repo-scoped Linear project ID under `.metastack/meta.json`"))?;
    let project = service
        .get_project(&project_id)
        .await
        .with_context(|| format!("failed to load Linear project `{project_id}`"))?;

    let can_prompt = io::stdin().is_terminal() && io::stdout().is_terminal();
    let run_non_interactive = args.no_interactive || !can_prompt;
    let request = resolve_request(run_non_interactive, args.request.as_deref())?;

    let run_id = OffsetDateTime::now_utc()
        .format(&format_description!("[year][month][day]T[hour][minute][second]Z"))
        .context("failed to format the roadmap run id")?;
    let paths = PlanningPaths::new(&root);
    let roadmap_runs_dir = paths.metastack_dir.join("roadmap-runs");
    ensure_dir(&roadmap_runs_dir)?;
    let run_dir = roadmap_runs_dir.join(&run_id);
    ensure_dir(&run_dir)?;

    let roadmap_path = root.join(ROADMAP_FILE_NAME);
    let existing_roadmap = fs::read_to_string(&roadmap_path).ok();

    let (manifest_entries, collected_sources) =
        collect_sources(&root, &roadmap_path, &args.sources, &project, &service).await?;
    let source_manifest_path = run_dir.join("source-manifest.json");
    fs::write(
        &source_manifest_path,
        serde_json::to_string_pretty(&manifest_entries)
            .context("failed to encode the roadmap source manifest")?,
    )
    .with_context(|| format!("failed to write `{}`", source_manifest_path.display()))?;

    let follow_up_questions = generate_follow_up_questions(&root, &request)?;
    let follow_up_answers = resolve_follow_up_answers(
        run_non_interactive,
        &follow_up_questions,
        &args.answers,
    )?;
    let proposal = generate_proposal(
        &root,
        &request,
        &follow_up_answers,
        &collected_sources,
        &project,
        existing_roadmap.as_deref(),
    )?;
    let proposal_path = run_dir.join("proposal.md");
    fs::write(&proposal_path, &proposal)
        .with_context(|| format!("failed to write `{}`", proposal_path.display()))?;

    let diff_path = if let Some(existing) = existing_roadmap.as_deref() {
        let rendered = render_text_diff("current roadmap", "proposal", existing, &proposal);
        let diff_path = run_dir.join("diff.md");
        fs::write(&diff_path, rendered)
            .with_context(|| format!("failed to write `{}`", diff_path.display()))?;
        Some(diff_path)
    } else {
        None
    };

    let current_linear_doc = project.description.clone().unwrap_or_default();
    let pre_sync_divergence = classify_divergence(
        existing_roadmap.as_deref(),
        current_linear_doc.as_str(),
        args.apply,
        &proposal,
    );

    let apply_result = if args.apply {
        Some(
            apply_proposal(
                &roadmap_path,
                &proposal,
                current_linear_doc.as_str(),
                &project,
                &service,
            )
            .await,
        )
    } else {
        None
    };
    let (repo_write_status, post_sync_divergence, apply_error) = match apply_result {
        Some(Ok((status, divergence))) => (status, divergence, None),
        Some(Err(error)) => (
            RoadmapWriteStatus::NotApplied,
            pre_sync_divergence,
            Some(error.to_string()),
        ),
        None => (
            RoadmapWriteStatus::NotApplied,
            pre_sync_divergence,
            None,
        ),
    };

    let summary = RoadmapRunSummary {
        run_id: run_id.clone(),
        route_key: AGENT_ROUTE_BACKLOG_ROADMAP.to_string(),
        roadmap_path: display_path(&roadmap_path, &root),
        proposal_path: display_path(&proposal_path, &root),
        source_manifest_path: display_path(&source_manifest_path, &root),
        diff_path: diff_path.as_ref().map(|path| display_path(path, &root)),
        apply_requested: args.apply,
        repo_write_status: repo_write_status.as_str().to_string(),
        pre_sync_divergence: pre_sync_divergence.as_str().to_string(),
        post_sync_divergence: post_sync_divergence.as_str().to_string(),
        project_id: project.id.clone(),
        project_name: project.name.clone(),
        sources_used: manifest_entries
            .iter()
            .filter(|entry| entry.status == "used")
            .count(),
        sources_skipped: manifest_entries
            .iter()
            .filter(|entry| entry.status == "skipped")
            .count(),
        sources_errored: manifest_entries
            .iter()
            .filter(|entry| entry.status == "error")
            .count(),
        apply_error: apply_error.clone(),
    };
    let summary_path = run_dir.join("summary.json");
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&summary)
            .context("failed to encode the roadmap run summary")?,
    )
    .with_context(|| format!("failed to write `{}`", summary_path.display()))?;

    println!(
        "roadmap run {}: proposal={}, repo={}, divergence={} -> {}",
        run_id,
        display_path(&proposal_path, &root),
        repo_write_status.as_str(),
        pre_sync_divergence.as_str(),
        post_sync_divergence.as_str()
    );
    if let Some(path) = diff_path {
        println!("diff artifact: {}", display_path(&path, &root));
    }
    println!(
        "artifacts: {}, {}",
        display_path(&source_manifest_path, &root),
        display_path(&summary_path, &root)
    );

    if let Some(error) = apply_error {
        bail!(error);
    }

    Ok(())
}

fn resolve_request(run_non_interactive: bool, request: Option<&str>) -> Result<String> {
    if let Some(request) = request {
        let trimmed = request.trim();
        if trimmed.is_empty() {
            bail!("`--request` cannot be empty");
        }
        return Ok(trimmed.to_string());
    }

    if run_non_interactive {
        bail!(
            "`--request` is required when `--no-interactive` is used or when `meta backlog roadmap` runs without a TTY"
        );
    }

    print!("Roadmap request: ");
    io::stdout()
        .flush()
        .context("failed to flush the roadmap prompt")?;
    let mut buffer = String::new();
    io::stdin()
        .read_line(&mut buffer)
        .context("failed to read the roadmap request")?;
    let trimmed = buffer.trim();
    if trimmed.is_empty() {
        bail!("roadmap request cannot be empty");
    }
    Ok(trimmed.to_string())
}

fn should_collect(entry: &DirEntry) -> bool {
    let file_name = entry.file_name().to_string_lossy();
    !(matches!(
        file_name.as_ref(),
        ".git" | "target" | "node_modules" | ".idea" | ".DS_Store"
    ) || entry.file_type().is_dir()
        && file_name == "roadmap-runs"
        && entry
            .path()
            .components()
            .any(|component| component.as_os_str() == ".metastack"))
}

async fn collect_sources<C>(
    root: &Path,
    roadmap_path: &Path,
    explicit_sources: &[PathBuf],
    project: &ProjectSummary,
    service: &LinearService<C>,
) -> Result<(Vec<SourceManifestEntry>, Vec<CollectedSource>)>
where
    C: LinearClient,
{
    let mut manifest = Vec::new();
    let mut sources = Vec::new();

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(should_collect)
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                manifest.push(SourceManifestEntry {
                    label: "discovery".to_string(),
                    origin: "discovered".to_string(),
                    path: None,
                    status: "error".to_string(),
                    reason: Some(error.to_string()),
                    bytes: None,
                });
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path == roadmap_path {
            continue;
        }
        if !matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("md" | "txt")
        ) {
            continue;
        }
        let relative = display_path(path, root);
        match collect_single_source(root, path, "discovered") {
            Ok(Some(source)) => {
                manifest.push(SourceManifestEntry {
                    label: relative.clone(),
                    origin: "discovered".to_string(),
                    path: Some(relative),
                    status: "used".to_string(),
                    reason: None,
                    bytes: Some(source.body.len()),
                });
                sources.push(source);
            }
            Ok(None) => {
                manifest.push(SourceManifestEntry {
                    label: relative.clone(),
                    origin: "discovered".to_string(),
                    path: Some(relative),
                    status: "skipped".to_string(),
                    reason: Some("source was empty after trimming".to_string()),
                    bytes: Some(0),
                });
            }
            Err(error) => {
                manifest.push(SourceManifestEntry {
                    label: relative.clone(),
                    origin: "discovered".to_string(),
                    path: Some(relative),
                    status: "error".to_string(),
                    reason: Some(error.to_string()),
                    bytes: None,
                });
            }
        }
    }

    for source in explicit_sources {
        match collect_single_source(root, source, "explicit") {
            Ok(Some(collected)) => {
                manifest.push(SourceManifestEntry {
                    label: collected.label.clone(),
                    origin: "explicit".to_string(),
                    path: Some(display_path(source, root)),
                    status: "used".to_string(),
                    reason: None,
                    bytes: Some(collected.body.len()),
                });
                sources.push(collected);
            }
            Ok(None) => {
                manifest.push(SourceManifestEntry {
                    label: display_path(source, root),
                    origin: "explicit".to_string(),
                    path: Some(display_path(source, root)),
                    status: "skipped".to_string(),
                    reason: Some("source was empty after trimming".to_string()),
                    bytes: Some(0),
                });
            }
            Err(error) => {
                manifest.push(SourceManifestEntry {
                    label: display_path(source, root),
                    origin: "explicit".to_string(),
                    path: Some(display_path(source, root)),
                    status: "error".to_string(),
                    reason: Some(error.to_string()),
                    bytes: None,
                });
            }
        }
    }

    let project_body = render_project_context(project);
    manifest.push(SourceManifestEntry {
        label: format!("Linear project {}", project.name),
        origin: "linear-project".to_string(),
        path: None,
        status: "used".to_string(),
        reason: None,
        bytes: Some(project_body.len()),
    });
    sources.push(CollectedSource {
        label: format!("Linear project {}", project.name),
        body: project_body,
    });

    match service
        .list_issues(IssueListFilters {
            project_id: Some(project.id.clone()),
            state: Some("Done".to_string()),
            limit: 25,
            ..IssueListFilters::default()
        })
        .await
    {
        Ok(issues) if issues.is_empty() => manifest.push(SourceManifestEntry {
            label: "Completed Linear issues".to_string(),
            origin: "linear-issues".to_string(),
            path: None,
            status: "skipped".to_string(),
            reason: Some("no completed issues matched the configured project".to_string()),
            bytes: Some(0),
        }),
        Ok(issues) => {
            let body = issues
                .iter()
                .map(|issue| format!("- {}: {}", issue.identifier, issue.title))
                .collect::<Vec<_>>()
                .join("\n");
            manifest.push(SourceManifestEntry {
                label: "Completed Linear issues".to_string(),
                origin: "linear-issues".to_string(),
                path: None,
                status: "used".to_string(),
                reason: None,
                bytes: Some(body.len()),
            });
            sources.push(CollectedSource {
                label: "Completed Linear issues".to_string(),
                body: format!("## Completed Linear Issues\n\n{body}\n"),
            });
        }
        Err(error) => manifest.push(SourceManifestEntry {
            label: "Completed Linear issues".to_string(),
            origin: "linear-issues".to_string(),
            path: None,
            status: "error".to_string(),
            reason: Some(error.to_string()),
            bytes: None,
        }),
    }

    match collect_merged_pr_evidence(root) {
        Ok(Some(body)) => {
            manifest.push(SourceManifestEntry {
                label: "Merged pull request evidence".to_string(),
                origin: "git".to_string(),
                path: None,
                status: "used".to_string(),
                reason: None,
                bytes: Some(body.len()),
            });
            sources.push(CollectedSource {
                label: "Merged pull request evidence".to_string(),
                body,
            });
        }
        Ok(None) => manifest.push(SourceManifestEntry {
            label: "Merged pull request evidence".to_string(),
            origin: "git".to_string(),
            path: None,
            status: "skipped".to_string(),
            reason: Some("no recent merge commits were found".to_string()),
            bytes: Some(0),
        }),
        Err(error) => manifest.push(SourceManifestEntry {
            label: "Merged pull request evidence".to_string(),
            origin: "git".to_string(),
            path: None,
            status: "error".to_string(),
            reason: Some(error.to_string()),
            bytes: None,
        }),
    }

    Ok((manifest, sources))
}

fn collect_single_source(
    root: &Path,
    path: &Path,
    _origin: &str,
) -> Result<Option<CollectedSource>> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("failed to resolve `{}`", path.display()))?;
    if !canonical.starts_with(root) {
        bail!(
            "source `{}` is outside the repository root `{}`",
            path.display(),
            root.display()
        );
    }
    if canonical
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|extension| !matches!(extension, "md" | "txt"))
    {
        bail!("source `{}` must end in `.md` or `.txt`", path.display());
    }

    let bytes = fs::read(&canonical)
        .with_context(|| format!("failed to read `{}`", canonical.display()))?;
    if bytes.len() > ROADMAP_MAX_SOURCE_BYTES {
        bail!(
            "source `{}` exceeds the {} byte limit",
            canonical.display(),
            ROADMAP_MAX_SOURCE_BYTES
        );
    }
    let body = String::from_utf8(bytes)
        .with_context(|| format!("failed to decode `{}` as UTF-8", canonical.display()))?;
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let relative = display_path(&canonical, root);
    Ok(Some(CollectedSource {
        label: relative.clone(),
        body: format!("## Source: {relative}\n\n{trimmed}\n"),
    }))
}

fn render_project_context(project: &ProjectSummary) -> String {
    let progress = project
        .progress
        .map(|value| format!("{:.0}%", value * 100.0))
        .unwrap_or_else(|| "unknown".to_string());
    let description = project
        .description
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("No current Linear project description.");
    format!(
        "## Linear Project\n\n- Name: {}\n- URL: {}\n- Progress: {}\n\n{}\n",
        project.name, project.url, progress, description
    )
}

fn collect_merged_pr_evidence(root: &Path) -> Result<Option<String>> {
    let root_display = root.display().to_string();
    let output = Command::new("git")
        .args([
            "-C",
            root_display.as_str(),
            "log",
            "--merges",
            "--pretty=format:%H\t%s",
            "-n",
            "10",
        ])
        .output()
        .context("failed to run `git log --merges`")?;
    if !output.status.success() {
        bail!(
            "`git log --merges` exited with status {}",
            output.status
        );
    }
    let stdout = String::from_utf8(output.stdout).context("git merge output was not UTF-8")?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!("## Recent Merged Pull Requests\n\n{}\n", trimmed)))
}

fn generate_follow_up_questions(root: &Path, request: &str) -> Result<Vec<String>> {
    let prompt = format!(
        "You are preparing one repository roadmap request.\nReturn strict JSON with this shape only: {{\"questions\":[\"...\"]}}.\nAsk at most {NON_INTERACTIVE_MAX_FOLLOW_UP_QUESTIONS} short follow-up questions.\nIf the request is already specific enough, return an empty array.\n\nRequest:\n{request}\n"
    );
    let report = run_agent_capture(&RunAgentArgs {
        root: Some(root.to_path_buf()),
        route_key: Some(AGENT_ROUTE_BACKLOG_ROADMAP.to_string()),
        agent: None,
        prompt,
        instructions: None,
        model: None,
        reasoning: None,
        transport: None,
        attachments: Vec::new(),
    })?;
    let parsed: serde_json::Value = serde_json::from_str(report.stdout.trim())
        .context("roadmap follow-up generation did not return valid JSON")?;
    let questions = parsed
        .get("questions")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow!("roadmap follow-up generation did not include `questions`"))?;
    Ok(questions
        .iter()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn resolve_follow_up_answers(
    run_non_interactive: bool,
    questions: &[String],
    provided_answers: &[String],
) -> Result<Vec<(String, String)>> {
    if questions.is_empty() {
        return Ok(Vec::new());
    }
    if run_non_interactive {
        if questions.len() != provided_answers.len() {
            bail!(
                "roadmap agent requested {} follow-up question(s); pass exactly {} `--answer` value(s)",
                questions.len(),
                questions.len()
            );
        }
        return Ok(questions
            .iter()
            .cloned()
            .zip(provided_answers.iter().cloned())
            .collect());
    }

    let mut answers = Vec::new();
    let stdin = io::stdin();
    for question in questions {
        print!("{question}: ");
        io::stdout()
            .flush()
            .context("failed to flush the roadmap follow-up prompt")?;
        let mut buffer = String::new();
        stdin
            .read_line(&mut buffer)
            .context("failed to read a roadmap follow-up answer")?;
        answers.push((question.clone(), buffer.trim().to_string()));
    }
    Ok(answers)
}

fn generate_proposal(
    root: &Path,
    request: &str,
    follow_ups: &[(String, String)],
    sources: &[CollectedSource],
    project: &ProjectSummary,
    existing_roadmap: Option<&str>,
) -> Result<String> {
    let rendered_follow_ups = if follow_ups.is_empty() {
        "No follow-up answers were collected.".to_string()
    } else {
        follow_ups
            .iter()
            .map(|(question, answer)| format!("- Q: {question}\n  A: {answer}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let rendered_sources = sources
        .iter()
        .map(|source| format!("{}\n", source.body))
        .collect::<Vec<_>>()
        .join("\n");
    let existing = existing_roadmap
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("No existing roadmap.md is present.");
    let prompt = format!(
        "You are drafting the canonical repository roadmap for project `{}`.\nReturn Markdown only.\nThe output must start with `## Summary` and include these exact headings in order:\n## Summary\n## Current State\n## Near-Term Workstreams\n## Risks And Dependencies\n## Validation\nDo not wrap the response in code fences.\n\nPrimary request:\n{}\n\nFollow-up answers:\n{}\n\nCurrent roadmap context:\n{}\n\nCollected sources:\n{}\n",
        project.name, request, rendered_follow_ups, existing, rendered_sources
    );
    let report = run_agent_capture(&RunAgentArgs {
        root: Some(root.to_path_buf()),
        route_key: Some(AGENT_ROUTE_BACKLOG_ROADMAP.to_string()),
        agent: None,
        prompt,
        instructions: None,
        model: None,
        reasoning: None,
        transport: None,
        attachments: Vec::new(),
    })?;
    Ok(normalize_proposal_markdown(report.stdout.trim()))
}

fn normalize_proposal_markdown(body: &str) -> String {
    let trimmed = body.trim();
    format!(
        "# Roadmap\n\n{ROADMAP_MARKER_START}\n\n{}\n\n{ROADMAP_MARKER_END}\n",
        trimmed
    )
}

fn classify_divergence(
    current_repo: Option<&str>,
    current_linear: &str,
    apply_requested: bool,
    proposed: &str,
) -> DivergenceStatus {
    let repo = current_repo.unwrap_or_default().trim();
    let linear = current_linear.trim();
    if repo == linear {
        return if apply_requested && proposed.trim() != linear {
            DivergenceStatus::RepoAhead
        } else {
            DivergenceStatus::InSync
        };
    }
    if repo.is_empty() && !linear.is_empty() {
        return DivergenceStatus::LinearAhead;
    }
    if linear.is_empty() && !repo.is_empty() {
        return DivergenceStatus::RepoAhead;
    }
    DivergenceStatus::LinearAhead
}

async fn apply_proposal<C>(
    roadmap_path: &Path,
    proposal: &str,
    current_linear_doc: &str,
    project: &ProjectSummary,
    service: &LinearService<C>,
) -> Result<(RoadmapWriteStatus, DivergenceStatus)>
where
    C: LinearClient,
{
    let existing_repo = fs::read_to_string(roadmap_path).ok().unwrap_or_default();
    if !current_linear_doc.trim().is_empty() && existing_repo.trim() != current_linear_doc.trim() {
        bail!(
            "refusing to overwrite a Linear-ahead project doc for `{}`; refresh the repo roadmap first",
            project.name
        );
    }

    let repo_write_status = match write_text_file(roadmap_path, proposal, true)? {
        crate::fs::FileWriteStatus::Created => RoadmapWriteStatus::Created,
        crate::fs::FileWriteStatus::Updated => RoadmapWriteStatus::Updated,
        crate::fs::FileWriteStatus::Unchanged => RoadmapWriteStatus::Unchanged,
    };

    service
        .update_project(
            &project.id,
            ProjectUpdateRequest {
                description: Some(proposal.to_string()),
            },
        )
        .await
        .with_context(|| format!("failed to sync roadmap content to Linear project `{}`", project.name))?;

    Ok((repo_write_status, DivergenceStatus::InSync))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Mutex};

    use anyhow::Result;
    use async_trait::async_trait;
    use tempfile::tempdir;

    use super::{
        DivergenceStatus, ROADMAP_MARKER_END, ROADMAP_MARKER_START, apply_proposal,
        classify_divergence, collect_single_source, normalize_proposal_markdown,
    };
    use crate::linear::{
        AttachmentCreateRequest, AttachmentSummary, IssueComment, IssueCreateRequest,
        IssueLabelCreateRequest, IssueListFilters, IssueSummary, IssueUpdateRequest, LabelRef,
        LinearClient, LinearService, ProjectSummary, ProjectUpdateRequest, TeamSummary, UserRef,
    };

    #[test]
    fn normalize_proposal_wraps_canonical_markers() {
        let proposal = normalize_proposal_markdown("## Summary\n\nBody");
        assert!(proposal.contains("# Roadmap"));
        assert!(proposal.contains(ROADMAP_MARKER_START));
        assert!(proposal.contains(ROADMAP_MARKER_END));
    }

    #[test]
    fn classify_divergence_prefers_repo_ahead_for_pending_apply() {
        assert_eq!(
            classify_divergence(
                Some("# Roadmap\nold"),
                "# Roadmap\nold",
                true,
                "# Roadmap\nnew"
            ),
            DivergenceStatus::RepoAhead
        );
        assert_eq!(
            classify_divergence(Some("# Roadmap\nold"), "# Roadmap\nremote", false, "# Roadmap\nnew"),
            DivergenceStatus::LinearAhead
        );
    }

    #[test]
    fn collect_single_source_rejects_paths_outside_repo() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let outside = temp.path().join("outside.md");
        fs::create_dir_all(&repo_root)?;
        fs::write(&outside, "outside")?;

        let error = collect_single_source(&repo_root, &outside, "explicit")
            .expect_err("outside path should fail");
        assert!(error.to_string().contains("outside the repository root"));
        Ok(())
    }

    #[derive(Clone)]
    struct RecordingProjectClient {
        seen_file_contents: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl LinearClient for RecordingProjectClient {
        async fn list_projects(&self, _limit: usize) -> Result<Vec<ProjectSummary>> {
            unreachable!()
        }

        async fn get_project(&self, _project_id: &str) -> Result<ProjectSummary> {
            unreachable!()
        }

        async fn list_users(&self, _limit: usize) -> Result<Vec<UserRef>> {
            unreachable!()
        }

        async fn list_issues(&self, _limit: usize) -> Result<Vec<IssueSummary>> {
            unreachable!()
        }

        async fn list_filtered_issues(
            &self,
            _filters: &IssueListFilters,
        ) -> Result<Vec<IssueSummary>> {
            unreachable!()
        }

        async fn list_issue_labels(&self, _team: Option<&str>) -> Result<Vec<LabelRef>> {
            unreachable!()
        }

        async fn get_issue(&self, _issue_id: &str) -> Result<IssueSummary> {
            unreachable!()
        }

        async fn list_teams(&self) -> Result<Vec<TeamSummary>> {
            unreachable!()
        }

        async fn viewer(&self) -> Result<UserRef> {
            unreachable!()
        }

        async fn create_issue(&self, _request: IssueCreateRequest) -> Result<IssueSummary> {
            unreachable!()
        }

        async fn create_issue_label(
            &self,
            _request: IssueLabelCreateRequest,
        ) -> Result<LabelRef> {
            unreachable!()
        }

        async fn update_project(
            &self,
            _project_id: &str,
            request: ProjectUpdateRequest,
        ) -> Result<ProjectSummary> {
            self.seen_file_contents
                .lock()
                .expect("mutex poisoned")
                .push(request.description.unwrap_or_default());
            Ok(ProjectSummary {
                id: "project-1".to_string(),
                name: "MetaStack CLI".to_string(),
                description: Some("updated".to_string()),
                url: "https://linear.app/project/1".to_string(),
                progress: Some(0.5),
                teams: Vec::new(),
            })
        }

        async fn update_issue(
            &self,
            _issue_id: &str,
            _request: IssueUpdateRequest,
        ) -> Result<IssueSummary> {
            unreachable!()
        }

        async fn create_comment(&self, _issue_id: &str, _body: String) -> Result<IssueComment> {
            unreachable!()
        }

        async fn update_comment(
            &self,
            _comment_id: &str,
            _body: String,
        ) -> Result<IssueComment> {
            unreachable!()
        }

        async fn upload_file(
            &self,
            _filename: &str,
            _content_type: &str,
            _contents: Vec<u8>,
        ) -> Result<String> {
            unreachable!()
        }

        async fn create_attachment(
            &self,
            _request: AttachmentCreateRequest,
        ) -> Result<AttachmentSummary> {
            unreachable!()
        }

        async fn delete_attachment(&self, _attachment_id: &str) -> Result<()> {
            unreachable!()
        }

        async fn download_file(&self, _url: &str) -> Result<Vec<u8>> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn apply_proposal_writes_repo_before_linear_sync() -> Result<()> {
        let temp = tempdir()?;
        let roadmap_path = temp.path().join("roadmap.md");
        let proposal = "# Roadmap\n\nnew";
        let recorder = Arc::new(Mutex::new(Vec::new()));
        let service = LinearService::new(
            RecordingProjectClient {
                seen_file_contents: recorder.clone(),
            },
            None,
        );
        let project = ProjectSummary {
            id: "project-1".to_string(),
            name: "MetaStack CLI".to_string(),
            description: Some(String::new()),
            url: "https://linear.app/project/1".to_string(),
            progress: Some(0.5),
            teams: Vec::new(),
        };

        let (_status, divergence) =
            apply_proposal(&roadmap_path, proposal, "", &project, &service).await?;

        assert_eq!(fs::read_to_string(&roadmap_path)?, proposal);
        assert_eq!(divergence, DivergenceStatus::InSync);
        assert_eq!(
            recorder.lock().expect("mutex poisoned").as_slice(),
            &[proposal.to_string()]
        );
        Ok(())
    }
}

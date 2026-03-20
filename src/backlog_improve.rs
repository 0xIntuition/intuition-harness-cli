use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::macros::format_description;

use crate::agents::run_agent_capture;
use crate::backlog::{
    BacklogIssueMetadata, INDEX_FILE_NAME, ManagedFileRecord, save_issue_metadata,
    write_issue_description,
};
use crate::cli::{BacklogImproveArgs, BacklogImproveModeArg, RunAgentArgs};
use crate::config::AGENT_ROUTE_BACKLOG_IMPROVE;
use crate::fs::{
    PlanningPaths, canonicalize_existing_dir, display_path, ensure_dir, write_text_file,
};
use crate::linear::{
    IssueEditSpec, IssueListFilters, IssueSummary, LinearService, ReqwestLinearClient,
};
use crate::repo_target::RepoTarget;
use crate::scaffold::ensure_planning_layout;
use crate::{LinearCommandContext, load_linear_command_context};

const ORIGINAL_SNAPSHOT_FILE: &str = "original.md";
const ISSUE_SNAPSHOT_FILE: &str = "issue.json";
const LOCAL_INDEX_SNAPSHOT_FILE: &str = "local-index.md";
const PROPOSAL_JSON_FILE: &str = "proposal.json";
const PROPOSAL_MARKDOWN_FILE: &str = "proposal.md";
const SUMMARY_FILE: &str = "summary.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ImprovementOutput {
    summary: String,
    #[serde(default)]
    needs_improvement: bool,
    #[serde(default)]
    findings: ImprovementFindings,
    #[serde(default)]
    proposal: ImprovementProposal,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ImprovementFindings {
    #[serde(default)]
    title_gaps: Vec<String>,
    #[serde(default)]
    description_gaps: Vec<String>,
    #[serde(default)]
    acceptance_criteria_gaps: Vec<String>,
    #[serde(default)]
    metadata_gaps: Vec<String>,
    #[serde(default)]
    structure_opportunities: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ImprovementProposal {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    priority: Option<u8>,
    #[serde(default)]
    estimate: Option<f64>,
    #[serde(default)]
    labels: Option<Vec<String>>,
    #[serde(default)]
    parent_issue_identifier: Option<String>,
    #[serde(default)]
    acceptance_criteria: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ImprovementApplyRecord {
    requested: bool,
    local_updated: bool,
    remote_updated: bool,
    local_before_path: Option<String>,
    local_after_path: Option<String>,
    remote_before_path: Option<String>,
    remote_after_path: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ImprovementRunSummary {
    run_id: String,
    issue_identifier: String,
    issue_title: String,
    mode: String,
    started_at: String,
    completed_at: String,
    needs_improvement: bool,
    original_snapshot_path: String,
    issue_snapshot_path: String,
    local_index_snapshot_path: Option<String>,
    proposal_json_path: String,
    proposal_markdown_path: String,
    apply: ImprovementApplyRecord,
}

#[derive(Debug, Clone)]
struct ImprovementReport {
    issue_identifier: String,
    run_dir: PathBuf,
    mode: BacklogImproveModeArg,
    needs_improvement: bool,
    apply_requested: bool,
}

/// Review repo-scoped backlog issues for hygiene gaps and optionally apply improvements.
///
/// Returns an error when repo planning metadata is missing, scoped issue discovery fails, the
/// configured agent cannot produce a valid proposal, or the requested Linear mutations fail.
pub async fn run_backlog_improve(args: &BacklogImproveArgs) -> Result<()> {
    let root = canonicalize_existing_dir(&args.client.root)?;
    ensure_planning_layout(&root, false)?;
    let command_context = load_linear_command_context(&args.client, None)?;
    let issues = load_target_issues(&command_context, args).await?;

    if issues.is_empty() {
        println!(
            "No repo-scoped issues matched state `{}` under the configured backlog scope.",
            args.state
        );
        return Ok(());
    }

    let related_backlog_issues = command_context
        .service
        .list_issues(IssueListFilters {
            team: command_context.default_team.clone(),
            project_id: command_context.default_project_id.clone(),
            state: Some(args.state.clone()),
            limit: args.limit.max(issues.len()).max(25),
            ..IssueListFilters::default()
        })
        .await?;

    let mut reports = Vec::with_capacity(issues.len());
    for issue in &issues {
        let report = improve_issue(
            &root,
            &command_context.service,
            issue,
            &related_backlog_issues,
            args,
        )
        .await?;
        reports.push(report);
    }

    println!("{}", render_improvement_reports(&root, &reports));
    Ok(())
}

async fn load_target_issues(
    command_context: &LinearCommandContext,
    args: &BacklogImproveArgs,
) -> Result<Vec<IssueSummary>> {
    if !args.issues.is_empty() {
        let mut issues = Vec::with_capacity(args.issues.len());
        for identifier in &args.issues {
            let issue = command_context.service.load_issue(identifier).await?;
            validate_issue_scope(
                &issue,
                command_context.default_team.as_deref(),
                command_context.default_project_id.as_deref(),
            )?;
            issues.push(issue);
        }
        return Ok(issues);
    }

    command_context
        .service
        .list_issues(IssueListFilters {
            team: command_context.default_team.clone(),
            project_id: command_context.default_project_id.clone(),
            state: Some(args.state.clone()),
            limit: args.limit.max(1),
            ..IssueListFilters::default()
        })
        .await
}

async fn improve_issue(
    root: &Path,
    service: &LinearService<ReqwestLinearClient>,
    issue: &IssueSummary,
    related_backlog_issues: &[IssueSummary],
    args: &BacklogImproveArgs,
) -> Result<ImprovementReport> {
    let started_at = now_rfc3339()?;
    let run_id = improvement_run_id()?;
    let paths = PlanningPaths::new(root);
    let issue_dir = paths.backlog_issue_dir(&issue.identifier);
    ensure_dir(&issue_dir)?;
    save_issue_metadata(&issue_dir, &build_issue_metadata(issue))?;

    let run_dir = issue_dir
        .join("artifacts")
        .join("improvement")
        .join(&run_id);
    ensure_dir(&run_dir)?;

    let original_description = issue.description.clone().unwrap_or_default();
    let original_snapshot_path = run_dir.join(ORIGINAL_SNAPSHOT_FILE);
    write_text_file(&original_snapshot_path, &original_description, true)?;

    let issue_snapshot_path = run_dir.join(ISSUE_SNAPSHOT_FILE);
    write_text_file(
        &issue_snapshot_path,
        &serde_json::to_string_pretty(issue).context("failed to encode issue snapshot")?,
        true,
    )?;

    let local_index_path = issue_dir.join(INDEX_FILE_NAME);
    let local_index_before = match fs::read_to_string(&local_index_path) {
        Ok(contents) => Some(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read `{}`", local_index_path.display()));
        }
    };
    let local_index_snapshot_path = if let Some(contents) = local_index_before.as_deref() {
        let path = run_dir.join(LOCAL_INDEX_SNAPSHOT_FILE);
        write_text_file(&path, contents, true)?;
        Some(path)
    } else {
        None
    };

    let prompt = render_improvement_prompt(
        root,
        issue,
        local_index_before.as_deref(),
        related_backlog_issues,
        args.mode,
    )?;
    let output = run_agent_capture(&RunAgentArgs {
        root: Some(root.to_path_buf()),
        route_key: Some(AGENT_ROUTE_BACKLOG_IMPROVE.to_string()),
        agent: args.agent.clone(),
        prompt,
        instructions: None,
        model: args.model.clone(),
        reasoning: args.reasoning.clone(),
        transport: None,
        attachments: Vec::new(),
    })
    .with_context(|| {
        "meta backlog improve requires a configured local agent to review repo-scoped backlog issues"
    })?;
    let parsed: ImprovementOutput =
        parse_agent_json(&output.stdout, "backlog improvement proposal")?;
    let normalized = normalize_improvement_output(issue, parsed)?;

    let proposal_json_path = run_dir.join(PROPOSAL_JSON_FILE);
    write_text_file(
        &proposal_json_path,
        &serde_json::to_string_pretty(&normalized)
            .context("failed to encode backlog improvement proposal")?,
        true,
    )?;
    let proposal_markdown_path = run_dir.join(PROPOSAL_MARKDOWN_FILE);
    write_text_file(
        &proposal_markdown_path,
        &render_proposal_markdown(args.mode, &normalized),
        true,
    )?;

    let mut apply = ImprovementApplyRecord {
        requested: args.apply,
        local_updated: false,
        remote_updated: false,
        local_before_path: None,
        local_after_path: None,
        remote_before_path: None,
        remote_after_path: None,
        error: None,
    };

    if args.apply && normalized.needs_improvement {
        let local_before_path = run_dir.join("applied-local-before.md");
        let local_after_path = run_dir.join("applied-local-after.md");
        let remote_before_path = run_dir.join("applied-remote-before.md");
        let remote_after_path = run_dir.join("applied-remote-after.md");

        write_text_file(
            &local_before_path,
            local_index_before.as_deref().unwrap_or_default(),
            true,
        )?;
        let proposed_description = normalized
            .proposal
            .description
            .clone()
            .unwrap_or_else(|| original_description.clone());
        write_text_file(&local_after_path, &proposed_description, true)?;
        write_text_file(&remote_before_path, &original_description, true)?;
        write_text_file(&remote_after_path, &proposed_description, true)?;

        apply.local_before_path = Some(display_path(&local_before_path, root));
        apply.local_after_path = Some(display_path(&local_after_path, root));
        apply.remote_before_path = Some(display_path(&remote_before_path, root));
        apply.remote_after_path = Some(display_path(&remote_after_path, root));

        if let Some(description) = normalized.proposal.description.as_deref() {
            write_issue_description(root, &issue.identifier, description)?;
            apply.local_updated = true;
        }

        if proposal_has_remote_mutation(&normalized.proposal) {
            let updated_issue = service
                .edit_issue(IssueEditSpec {
                    identifier: issue.identifier.clone(),
                    title: normalized.proposal.title.clone(),
                    description: normalized.proposal.description.clone(),
                    project: None,
                    state: None,
                    priority: normalized.proposal.priority,
                    estimate: normalized.proposal.estimate,
                    labels: normalized.proposal.labels.clone(),
                    parent_identifier: normalized.proposal.parent_issue_identifier.clone(),
                })
                .await;

            match updated_issue {
                Ok(updated_issue) => {
                    save_issue_metadata(&issue_dir, &build_issue_metadata(&updated_issue))?;
                    apply.remote_updated = true;
                }
                Err(error) => {
                    apply.error = Some(error.to_string());
                }
            }
        }
    }

    let completed_at = now_rfc3339()?;
    let summary = ImprovementRunSummary {
        run_id: run_id.clone(),
        issue_identifier: issue.identifier.clone(),
        issue_title: issue.title.clone(),
        mode: render_mode(args.mode).to_string(),
        started_at,
        completed_at,
        needs_improvement: normalized.needs_improvement,
        original_snapshot_path: display_path(&original_snapshot_path, root),
        issue_snapshot_path: display_path(&issue_snapshot_path, root),
        local_index_snapshot_path: local_index_snapshot_path
            .as_ref()
            .map(|path| display_path(path, root)),
        proposal_json_path: display_path(&proposal_json_path, root),
        proposal_markdown_path: display_path(&proposal_markdown_path, root),
        apply,
    };

    let summary_path = run_dir.join(SUMMARY_FILE);
    write_text_file(
        &summary_path,
        &serde_json::to_string_pretty(&summary)
            .context("failed to encode backlog improvement summary")?,
        true,
    )?;

    if let Some(error) = summary.apply.error.as_deref() {
        bail!(
            "improved `{}` but failed to finish the apply-back flow: {}",
            issue.identifier,
            error
        );
    }

    Ok(ImprovementReport {
        issue_identifier: issue.identifier.clone(),
        run_dir,
        mode: args.mode,
        needs_improvement: normalized.needs_improvement,
        apply_requested: args.apply,
    })
}

fn render_improvement_reports(root: &Path, reports: &[ImprovementReport]) -> String {
    let mut lines = vec![format!("Improved {} issue(s):", reports.len())];

    for report in reports {
        let status = if !report.needs_improvement {
            "no changes needed"
        } else if report.apply_requested {
            "applied"
        } else {
            "proposal only"
        };
        lines.push(format!(
            "- {}: {} {} ({})",
            report.issue_identifier,
            render_mode(report.mode),
            status,
            display_path(&report.run_dir, root)
        ));
    }

    lines.join("\n")
}

fn normalize_improvement_output(
    issue: &IssueSummary,
    output: ImprovementOutput,
) -> Result<ImprovementOutput> {
    let priority = if let Some(priority) = output.proposal.priority {
        if priority > 4 {
            bail!("backlog improvement proposed invalid priority `{priority}`");
        }
        Some(priority)
    } else {
        None
    };
    let estimate = match output.proposal.estimate {
        Some(estimate) if !estimate.is_finite() || estimate.is_sign_negative() => {
            bail!("backlog improvement proposed invalid estimate `{estimate}`");
        }
        other => other,
    };
    let parent_issue_identifier = output
        .proposal
        .parent_issue_identifier
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if parent_issue_identifier
        .as_deref()
        .is_some_and(|identifier| issue.identifier.eq_ignore_ascii_case(identifier))
    {
        bail!("backlog improvement proposed the issue as its own parent");
    }

    let labels = output
        .proposal
        .labels
        .map(normalize_string_list)
        .filter(|labels| !labels.is_empty());
    let acceptance_criteria = normalize_string_list(output.proposal.acceptance_criteria);
    let title = normalize_optional_text(output.proposal.title);
    let description = normalize_optional_text(output.proposal.description);

    let needs_improvement = output.needs_improvement
        || title.is_some()
        || description.is_some()
        || priority.is_some()
        || estimate.is_some()
        || labels.is_some()
        || parent_issue_identifier.is_some()
        || !acceptance_criteria.is_empty()
        || findings_present(&output.findings);

    Ok(ImprovementOutput {
        summary: trimmed_or_default(output.summary, "No summary provided."),
        needs_improvement,
        findings: ImprovementFindings {
            title_gaps: normalize_string_list(output.findings.title_gaps),
            description_gaps: normalize_string_list(output.findings.description_gaps),
            acceptance_criteria_gaps: normalize_string_list(
                output.findings.acceptance_criteria_gaps,
            ),
            metadata_gaps: normalize_string_list(output.findings.metadata_gaps),
            structure_opportunities: normalize_string_list(output.findings.structure_opportunities),
        },
        proposal: ImprovementProposal {
            title,
            description,
            priority,
            estimate,
            labels,
            parent_issue_identifier,
            acceptance_criteria,
        },
    })
}

fn findings_present(findings: &ImprovementFindings) -> bool {
    !findings.title_gaps.is_empty()
        || !findings.description_gaps.is_empty()
        || !findings.acceptance_criteria_gaps.is_empty()
        || !findings.metadata_gaps.is_empty()
        || !findings.structure_opportunities.is_empty()
}

fn proposal_has_remote_mutation(proposal: &ImprovementProposal) -> bool {
    proposal.title.is_some()
        || proposal.description.is_some()
        || proposal.priority.is_some()
        || proposal.estimate.is_some()
        || proposal.labels.is_some()
        || proposal.parent_issue_identifier.is_some()
}

fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty()
            || normalized
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(trimmed))
        {
            continue;
        }
        normalized.push(trimmed.to_string());
    }
    normalized
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn render_mode(mode: BacklogImproveModeArg) -> &'static str {
    match mode {
        BacklogImproveModeArg::Basic => "basic",
        BacklogImproveModeArg::Advanced => "advanced",
    }
}

fn render_proposal_markdown(mode: BacklogImproveModeArg, output: &ImprovementOutput) -> String {
    let mut lines = vec![
        "# Backlog Improvement Proposal".to_string(),
        String::new(),
        format!("- Mode: `{}`", render_mode(mode)),
        format!("- Needs improvement: `{}`", output.needs_improvement),
        String::new(),
        "## Summary".to_string(),
        String::new(),
        output.summary.clone(),
        String::new(),
    ];

    for (title, values) in [
        ("Title Gaps", &output.findings.title_gaps),
        ("Description Gaps", &output.findings.description_gaps),
        (
            "Acceptance Criteria Gaps",
            &output.findings.acceptance_criteria_gaps,
        ),
        ("Metadata Gaps", &output.findings.metadata_gaps),
        (
            "Structure Opportunities",
            &output.findings.structure_opportunities,
        ),
    ] {
        lines.push(format!("## {title}"));
        lines.push(String::new());
        if values.is_empty() {
            lines.push("- None identified.".to_string());
        } else {
            lines.extend(values.iter().map(|value| format!("- {value}")));
        }
        lines.push(String::new());
    }

    lines.push("## Proposed Changes".to_string());
    lines.push(String::new());
    lines.push(format!(
        "- Title: {}",
        output.proposal.title.as_deref().unwrap_or("_unchanged_")
    ));
    lines.push(format!(
        "- Priority: {}",
        output
            .proposal
            .priority
            .map(|value| value.to_string())
            .unwrap_or_else(|| "_unchanged_".to_string())
    ));
    lines.push(format!(
        "- Estimate: {}",
        output
            .proposal
            .estimate
            .map(|value| value.to_string())
            .unwrap_or_else(|| "_unchanged_".to_string())
    ));
    lines.push(format!(
        "- Parent issue: {}",
        output
            .proposal
            .parent_issue_identifier
            .as_deref()
            .unwrap_or("_unchanged_")
    ));
    lines.push(format!(
        "- Labels: {}",
        output
            .proposal
            .labels
            .as_ref()
            .map(|labels| labels.join(", "))
            .unwrap_or_else(|| "_unchanged_".to_string())
    ));
    lines.push(String::new());
    lines.push("### Acceptance Criteria".to_string());
    lines.push(String::new());
    if output.proposal.acceptance_criteria.is_empty() {
        lines.push("- _No explicit acceptance-criteria rewrite proposed._".to_string());
    } else {
        lines.extend(
            output
                .proposal
                .acceptance_criteria
                .iter()
                .map(|value| format!("- {value}")),
        );
    }
    lines.push(String::new());
    lines.push("### Description".to_string());
    lines.push(String::new());
    lines.push(
        output
            .proposal
            .description
            .clone()
            .unwrap_or_else(|| "_No description rewrite proposed._".to_string()),
    );
    lines.join("\n")
}

fn render_improvement_prompt(
    root: &Path,
    issue: &IssueSummary,
    local_index_snapshot: Option<&str>,
    related_backlog_issues: &[IssueSummary],
    mode: BacklogImproveModeArg,
) -> Result<String> {
    let repo_target = RepoTarget::from_root(root);
    let planning_context = load_context_bundle(root)?;
    let current_description = issue
        .description
        .as_deref()
        .unwrap_or("_No Linear description was provided._");
    let local_backlog_block = local_index_snapshot
        .map(str::trim)
        .filter(|contents| !contents.is_empty())
        .map(|contents| render_fenced_block("md", contents))
        .unwrap_or_else(|| "_No local backlog packet exists yet for this issue._".to_string());
    let related_backlog_block = render_related_backlog_catalog(issue, related_backlog_issues);

    Ok(format!(
        "You are improving the quality of an existing repo-scoped backlog issue.\n\n\
Repository scope:\n{repo_scope}\n\n\
Improvement mode: `{mode}`\n\
- `basic`: keep edits conservative and focus on labels, title hygiene, missing acceptance criteria, priority, estimate, and small description cleanups.\n\
- `advanced`: you may rewrite title/description more deeply and recommend or assign an existing parent issue when the work clearly belongs in a parent-child structure.\n\n\
Issue metadata:\n\
- Identifier: `{identifier}`\n\
- Title: {title}\n\
- Team: {team}\n\
- Project: {project}\n\
- State: {state}\n\
- Priority: {priority}\n\
- Estimate: {estimate}\n\
- Labels: {labels}\n\
- Parent: {parent}\n\
- Children: {children}\n\
- URL: {url}\n\n\
Current Linear description:\n{current_description_block}\n\n\
Current local backlog index snapshot:\n{local_backlog_block}\n\n\
Related repo-scoped backlog issues:\n{related_backlog_block}\n\n\
Repository planning context:\n{planning_context}\n\n\
Instructions:\n\
1. Decide whether this issue needs improvement before execution.\n\
2. Inspect issue hygiene gaps: weak title, weak description, missing acceptance criteria, absent or unclear labels, missing priority/estimate, and opportunities to group work under an existing parent issue.\n\
3. Stay inside the provided repository scope. Do not invent cross-repo work or new storage models.\n\
4. When you propose a parent issue, choose only from the provided related backlog issue catalog and only when the relationship is strong.\n\
5. When you propose description changes, return the full Markdown description ready for `.metastack/backlog/<ISSUE>/index.md`.\n\
6. In `basic` mode, prefer modest rewrites and safe metadata cleanup. In `advanced` mode, you may rewrite more substantially and use structure changes when justified.\n\
7. Return JSON only using this exact shape:\n\
{{\n\
  \"summary\": \"One paragraph explaining the main improvement judgment\",\n\
  \"needs_improvement\": true,\n\
  \"findings\": {{\n\
    \"title_gaps\": [\"...\"],\n\
    \"description_gaps\": [\"...\"],\n\
    \"acceptance_criteria_gaps\": [\"...\"],\n\
    \"metadata_gaps\": [\"...\"],\n\
    \"structure_opportunities\": [\"...\"]\n\
  }},\n\
  \"proposal\": {{\n\
    \"title\": \"Optional replacement title\",\n\
    \"description\": \"Optional full Markdown rewrite\",\n\
    \"priority\": 2,\n\
    \"estimate\": 3,\n\
    \"labels\": [\"plan\", \"technical\"],\n\
    \"parent_issue_identifier\": \"ENG-10001\",\n\
    \"acceptance_criteria\": [\"...\"]\n\
  }}\n\
}}",
        repo_scope = repo_target.prompt_scope_block(),
        mode = render_mode(mode),
        identifier = issue.identifier,
        title = issue.title,
        team = issue.team.key,
        project = issue
            .project
            .as_ref()
            .map(|project| project.name.as_str())
            .unwrap_or("No project"),
        state = issue
            .state
            .as_ref()
            .map(|state| state.name.as_str())
            .unwrap_or("Unknown"),
        priority = issue
            .priority
            .map(|value| value.to_string())
            .unwrap_or_else(|| "None".to_string()),
        estimate = issue
            .estimate
            .map(|value| value.to_string())
            .unwrap_or_else(|| "None".to_string()),
        labels = render_labels(issue),
        parent = issue
            .parent
            .as_ref()
            .map(|parent| parent.identifier.as_str())
            .unwrap_or("None"),
        children = issue.children.len(),
        url = issue.url,
        current_description_block = render_fenced_block("md", current_description),
    ))
}

fn render_related_backlog_catalog(
    issue: &IssueSummary,
    related_backlog_issues: &[IssueSummary],
) -> String {
    let mut entries = related_backlog_issues
        .iter()
        .filter(|candidate| candidate.identifier != issue.identifier)
        .map(|candidate| {
            format!(
                "- `{}` | {} | parent={} | labels={} | title={}",
                candidate.identifier,
                candidate
                    .state
                    .as_ref()
                    .map(|state| state.name.as_str())
                    .unwrap_or("Unknown"),
                candidate
                    .parent
                    .as_ref()
                    .map(|parent| parent.identifier.as_str())
                    .unwrap_or("none"),
                render_labels(candidate),
                candidate.title
            )
        })
        .collect::<Vec<_>>();

    if entries.is_empty() {
        "_No other repo-scoped backlog issues were available for structure comparison._".to_string()
    } else {
        entries.truncate(25);
        entries.join("\n")
    }
}

fn render_labels(issue: &IssueSummary) -> String {
    if issue.labels.is_empty() {
        "none".to_string()
    } else {
        issue
            .labels
            .iter()
            .map(|label| label.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn validate_issue_scope(
    issue: &IssueSummary,
    default_team: Option<&str>,
    default_project_id: Option<&str>,
) -> Result<()> {
    if let Some(team) = default_team
        && !issue.team.key.eq_ignore_ascii_case(team)
    {
        bail!(
            "issue `{}` belongs to team `{}`, outside the configured repo team scope `{}`",
            issue.identifier,
            issue.team.key,
            team
        );
    }

    if let Some(project_selector) = default_project_id {
        let Some(project) = issue.project.as_ref() else {
            bail!(
                "issue `{}` has no project, outside the configured repo project scope `{}`",
                issue.identifier,
                project_selector
            );
        };
        let matches =
            project.id == project_selector || project.name.eq_ignore_ascii_case(project_selector);
        if !matches {
            bail!(
                "issue `{}` belongs to project `{}` (`{}`), outside the configured repo project scope `{}`",
                issue.identifier,
                project.name,
                project.id,
                project_selector
            );
        }
    }

    Ok(())
}

fn build_issue_metadata(issue: &IssueSummary) -> BacklogIssueMetadata {
    BacklogIssueMetadata {
        issue_id: issue.id.clone(),
        identifier: issue.identifier.clone(),
        title: issue.title.clone(),
        url: issue.url.clone(),
        team_key: issue.team.key.clone(),
        project_id: issue.project.as_ref().map(|project| project.id.clone()),
        project_name: issue.project.as_ref().map(|project| project.name.clone()),
        parent_id: issue.parent.as_ref().map(|parent| parent.id.clone()),
        parent_identifier: issue
            .parent
            .as_ref()
            .map(|parent| parent.identifier.clone()),
        local_hash: None,
        remote_hash: None,
        last_sync_at: None,
        last_pulled_comment_ids: Vec::new(),
        managed_files: Vec::<ManagedFileRecord>::new(),
    }
}

fn render_fenced_block(language: &str, contents: &str) -> String {
    let fence_len = max_backtick_run(contents).saturating_add(1).max(3);
    let fence = "`".repeat(fence_len);
    if language.is_empty() {
        format!("{fence}\n{contents}\n{fence}")
    } else {
        format!("{fence}{language}\n{contents}\n{fence}")
    }
}

fn max_backtick_run(value: &str) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for ch in value.chars() {
        if ch == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

fn load_context_bundle(root: &Path) -> Result<String> {
    let paths = PlanningPaths::new(root);
    let sections = [
        ("SCAN.md", paths.scan_path()),
        ("ARCHITECTURE.md", paths.architecture_path()),
        ("CONVENTIONS.md", paths.conventions_path()),
        ("STACK.md", paths.stack_path()),
        ("TESTING.md", paths.testing_path()),
    ];
    let mut lines = Vec::new();
    for (title, path) in sections {
        lines.push(format!("## {title}"));
        lines.push(String::new());
        lines.push(read_context(&path)?);
        lines.push(String::new());
    }
    Ok(lines.join("\n"))
}

fn read_context(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(format!(
            "_Missing `{}`. Run `meta scan` to generate it._",
            path.file_name()
                .map(|value| value.to_string_lossy())
                .unwrap_or_default()
        )),
        Err(error) => Err(error).with_context(|| format!("failed to read `{}`", path.display())),
    }
}

fn parse_agent_json<T>(raw: &str, phase: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let trimmed = raw.trim();
    let mut candidates = vec![trimmed.to_string()];
    if let Some(stripped) = strip_code_fence(trimmed) {
        candidates.push(stripped);
    }
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}'))
        && start <= end
    {
        candidates.push(trimmed[start..=end].to_string());
    }

    for candidate in candidates {
        if let Ok(parsed) = serde_json::from_str::<T>(&candidate) {
            return Ok(parsed);
        }
    }

    bail!(
        "backlog improvement agent returned invalid JSON during {phase}: {}",
        preview_text(trimmed)
    )
}

fn strip_code_fence(raw: &str) -> Option<String> {
    let stripped = raw.strip_prefix("```")?;
    let stripped = stripped
        .strip_prefix("json\n")
        .or_else(|| stripped.strip_prefix("JSON\n"))
        .or_else(|| stripped.strip_prefix('\n'))
        .unwrap_or(stripped);
    let stripped = stripped.strip_suffix("```")?;
    Some(stripped.trim().to_string())
}

fn preview_text(value: &str) -> String {
    const MAX_PREVIEW_CHARS: usize = 240;
    let Some((truncate_at, _)) = value.char_indices().nth(MAX_PREVIEW_CHARS) else {
        return value.to_string();
    };
    format!("{}...", &value[..truncate_at])
}

fn trimmed_or_default(value: String, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn now_rfc3339() -> Result<String> {
    OffsetDateTime::now_utc()
        .format(&format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second]Z"
        ))
        .context("failed to format the backlog improvement timestamp")
}

fn improvement_run_id() -> Result<String> {
    let now = OffsetDateTime::now_utc();
    let base = now
        .format(&format_description!(
            "[year][month][day]T[hour][minute][second]Z"
        ))
        .context("failed to format the backlog improvement run id")?;
    Ok(format!("{}-{:09}", base, now.nanosecond()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linear::{IssueLink, ProjectRef, TeamRef, WorkflowState};

    #[test]
    fn render_improvement_prompt_uses_safe_fences_for_existing_markdown_code_blocks() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path();
        let paths = PlanningPaths::new(root);
        write_text_file(&paths.scan_path(), "scan", true).expect("scan context");
        write_text_file(&paths.architecture_path(), "architecture", true).expect("architecture");
        write_text_file(&paths.conventions_path(), "conventions", true).expect("conventions");
        write_text_file(&paths.stack_path(), "stack", true).expect("stack");
        write_text_file(&paths.testing_path(), "testing", true).expect("testing");

        let issue = IssueSummary {
            id: "issue-1".to_string(),
            identifier: "ENG-10170".to_string(),
            title: "Improve prompt fences".to_string(),
            description: Some("```bash\ncargo test\n```".to_string()),
            url: "https://linear.example/ENG-10170".to_string(),
            priority: None,
            estimate: None,
            updated_at: "2026-03-20T00:00:00Z".to_string(),
            team: TeamRef {
                id: "team-1".to_string(),
                key: "ENG".to_string(),
                name: "Engineering".to_string(),
            },
            project: Some(ProjectRef {
                id: "project-1".to_string(),
                name: "MetaStack CLI".to_string(),
            }),
            assignee: None,
            labels: Vec::new(),
            comments: Vec::new(),
            state: Some(WorkflowState {
                id: "state-1".to_string(),
                name: "Backlog".to_string(),
                kind: Some("unstarted".to_string()),
            }),
            attachments: Vec::new(),
            parent: Some(IssueLink {
                id: "issue-parent".to_string(),
                identifier: "ENG-10100".to_string(),
                title: "Parent".to_string(),
                url: "https://linear.example/ENG-10100".to_string(),
                description: None,
            }),
            children: Vec::new(),
        };

        let prompt = render_improvement_prompt(
            root,
            &issue,
            Some("```md\n## Local\n```"),
            &[],
            BacklogImproveModeArg::Advanced,
        )
        .expect("prompt");

        assert!(prompt.contains("Current Linear description:\n````md\n```bash"));
        assert!(prompt.contains("Current local backlog index snapshot:\n````md\n```md"));
    }

    #[test]
    fn normalize_improvement_output_dedupes_labels_and_marks_changes() {
        let issue = IssueSummary {
            id: "issue-1".to_string(),
            identifier: "ENG-10170".to_string(),
            title: "Improve".to_string(),
            description: None,
            url: "https://linear.example/ENG-10170".to_string(),
            priority: None,
            estimate: None,
            updated_at: "2026-03-20T00:00:00Z".to_string(),
            team: TeamRef {
                id: "team-1".to_string(),
                key: "ENG".to_string(),
                name: "Engineering".to_string(),
            },
            project: None,
            assignee: None,
            labels: Vec::new(),
            comments: Vec::new(),
            state: None,
            attachments: Vec::new(),
            parent: None,
            children: Vec::new(),
        };

        let normalized = normalize_improvement_output(
            &issue,
            ImprovementOutput {
                summary: "  summary  ".to_string(),
                needs_improvement: false,
                findings: ImprovementFindings::default(),
                proposal: ImprovementProposal {
                    labels: Some(vec![
                        "plan".to_string(),
                        " Plan ".to_string(),
                        "technical".to_string(),
                    ]),
                    acceptance_criteria: vec![" first ".to_string(), "first".to_string()],
                    ..ImprovementProposal::default()
                },
            },
        )
        .expect("normalize");

        assert!(normalized.needs_improvement);
        assert_eq!(
            normalized.proposal.labels,
            Some(vec!["plan".to_string(), "technical".to_string()])
        );
        assert_eq!(
            normalized.proposal.acceptance_criteria,
            vec!["first".to_string()]
        );
        assert_eq!(normalized.summary, "summary");
    }
}

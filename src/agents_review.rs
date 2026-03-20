use std::collections::BTreeSet;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::agents::{
    apply_invocation_environment, apply_noninteractive_agent_environment,
    command_args_for_invocation, render_invocation_diagnostics,
    resolve_agent_invocation_for_planning, validate_invocation_command_surface,
};
use crate::cli::{AgentsReviewArgs, RunAgentArgs};
use crate::config::{AGENT_ROUTE_AGENTS_REVIEW, AppConfig, PlanningMeta};
use crate::context::{
    load_codebase_context_bundle, load_project_rules_bundle, load_workflow_contract,
    render_repo_map,
};
use crate::fs::{
    PlanningPaths, canonicalize_existing_dir, ensure_dir, ensure_workspace_path_is_safe,
    sibling_workspace_root,
};
use crate::linear::{
    LinearService, ReqwestLinearClient, TicketDiscussionBudgets, load_linear_command_context,
    prepare_issue_context,
};

const AUDIT_PROMPT_ARTIFACT: &str = include_str!("artifacts/agents/pr-audit-review.md");
const REMEDIATION_PROMPT_ARTIFACT: &str = include_str!("artifacts/agents/pr-audit-remediation.md");
const REVIEW_COMMENT_MARKER: &str = "## Meta Agents Review";

#[derive(Debug, Clone, Deserialize)]
struct GhRepositoryView {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
    url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GhPullRequestView {
    number: u64,
    title: String,
    body: String,
    url: String,
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    #[serde(rename = "baseRefName")]
    base_ref_name: String,
    #[serde(rename = "headRefOid")]
    head_ref_oid: String,
    #[serde(rename = "reviewDecision")]
    review_decision: Option<String>,
    #[serde(rename = "changedFiles")]
    changed_files: u64,
    additions: u64,
    deletions: u64,
    author: GhActor,
    files: Vec<GhChangedFile>,
    comments: Vec<GhIssueComment>,
    reviews: Vec<GhReview>,
}

#[derive(Debug, Clone, Deserialize)]
struct GhActor {
    login: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GhChangedFile {
    path: String,
    additions: u64,
    deletions: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct GhIssueComment {
    author: GhActor,
    body: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GhReview {
    author: GhActor,
    body: String,
    state: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GhInlineReviewComment {
    path: String,
    body: String,
    #[serde(default)]
    diff_hunk: Option<String>,
    user: GhActor,
}

#[derive(Debug, Clone, Deserialize)]
struct CreatedPullRequest {
    url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuditFinding {
    title: String,
    rationale: String,
    evidence: Vec<String>,
    #[serde(default)]
    suggested_fix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuditReport {
    summary: String,
    requires_follow_up_changes: bool,
    blocking_issues: Vec<AuditFinding>,
    optional_improvements: Vec<AuditFinding>,
    broader_impact_areas: Vec<String>,
    suggested_validation: Vec<String>,
    #[serde(default)]
    remediation_summary: Option<String>,
}

#[derive(Debug, Clone)]
struct ReviewContext {
    repository: GhRepositoryView,
    pull_request: GhPullRequestView,
    diff: String,
    inline_comments: Vec<GhInlineReviewComment>,
    issue: crate::linear::IssueSummary,
    issue_discussion: String,
    active_workpad: Option<String>,
    acceptance_criteria: Vec<String>,
    local_backlog_context: String,
    codebase_context: String,
    project_rules: String,
    workflow_contract: String,
    repo_map: String,
    repo_context_warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct RemediationOutcome {
    branch: String,
    commit: String,
    pull_request_url: String,
    agent_summary: String,
}

#[derive(Debug, Clone, Copy)]
struct GhCli;

/// Audit one GitHub pull request against its linked Linear ticket and optionally open a
/// remediation pull request when follow-up work is required.
///
/// Returns an error when repository resolution fails, GitHub/Linear context cannot be loaded, the
/// audit output is invalid, or any required remediation publication step fails.
pub async fn run_agents_review(args: &AgentsReviewArgs) -> Result<String> {
    let root = canonicalize_existing_dir(&args.client.root)?;
    if args.pull_request == 0 {
        bail!("pull request number must be greater than zero");
    }

    let review_context = assemble_review_context(&root, args).await?;
    let audit_prompt = build_audit_prompt(&review_context);
    let diagnostics = resolve_review_diagnostics(&root, args, &audit_prompt)?;

    if args.dry_run {
        return Ok(render_dry_run(
            &root,
            args.pull_request,
            &review_context,
            &diagnostics,
            &audit_prompt,
        ));
    }

    let audit_output = run_agent_capture_in_dir(&root, &root, args, &audit_prompt)?;
    let report = parse_json_block::<AuditReport>(&audit_output)
        .context("PR audit agent did not return a valid JSON report")?;

    if !report.requires_follow_up_changes {
        return Ok(render_no_fix_result(&review_context, &diagnostics, &report));
    }

    let remediation = apply_remediation(&root, args, &review_context, &report).await?;
    Ok(render_remediation_result(
        &review_context,
        &diagnostics,
        &report,
        &remediation,
    ))
}

async fn assemble_review_context(root: &Path, args: &AgentsReviewArgs) -> Result<ReviewContext> {
    let gh = GhCli;
    let repository = gh.resolve_repository(root)?;
    let pull_request = gh.view_pull_request(root, args.pull_request)?;
    let diff = gh.pull_request_diff(root, args.pull_request)?;
    let inline_comments =
        gh.pull_request_inline_comments(root, &repository.name_with_owner, args.pull_request)?;

    let linear_context = load_linear_command_context(&args.client, None)?;
    let issue = resolve_linked_issue(&linear_context.service, &pull_request).await?;
    let prepared = prepare_issue_context(&issue, TicketDiscussionBudgets::default());
    let active_workpad = issue
        .comments
        .iter()
        .find(|comment| comment.resolved_at.is_none() && comment.body.contains("## Codex Workpad"))
        .map(|comment| comment.body.clone());

    Ok(ReviewContext {
        repository,
        pull_request,
        diff,
        inline_comments,
        issue,
        issue_discussion: prepared.prompt_discussion,
        active_workpad,
        acceptance_criteria: extract_acceptance_criteria(prepared.issue.description.as_deref()),
        local_backlog_context: render_local_backlog_context(root, &prepared.issue.identifier)?,
        codebase_context: load_codebase_context_bundle(root)?,
        project_rules: load_project_rules_bundle(root)?,
        workflow_contract: load_workflow_contract(root)?,
        repo_map: render_repo_map(root)?,
        repo_context_warnings: repo_context_warnings(root),
    })
}

async fn resolve_linked_issue(
    service: &LinearService<ReqwestLinearClient>,
    pull_request: &GhPullRequestView,
) -> Result<crate::linear::IssueSummary> {
    let mut identifiers = BTreeSet::new();
    for source in [
        &pull_request.title,
        &pull_request.body,
        &pull_request.head_ref_name,
    ] {
        for identifier in extract_linear_identifiers(source) {
            identifiers.insert(identifier);
        }
    }

    if identifiers.is_empty() {
        bail!(
            "pull request #{} does not contain a recognizable Linear issue identifier in its title, body, or head branch",
            pull_request.number
        );
    }

    let mut matches = Vec::new();
    for identifier in identifiers {
        if let Ok(issue) = service.load_issue(&identifier).await {
            matches.push(issue);
        }
    }

    match matches.len() {
        0 => bail!(
            "pull request #{} did not resolve to a Linear issue. Add exactly one Linear identifier to the PR title, body, or head branch.",
            pull_request.number
        ),
        1 => Ok(matches.remove(0)),
        _ => bail!(
            "pull request #{} resolved to multiple Linear issues ({}). Narrow the PR linkage to one issue identifier.",
            pull_request.number,
            matches
                .iter()
                .map(|issue| issue.identifier.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn resolve_review_diagnostics(
    root: &Path,
    args: &AgentsReviewArgs,
    prompt: &str,
) -> Result<Vec<String>> {
    let config = AppConfig::load()?;
    let planning_meta = PlanningMeta::load(root)?;
    let invocation = resolve_agent_invocation_for_planning(
        &config,
        &planning_meta,
        &RunAgentArgs {
            root: Some(root.to_path_buf()),
            route_key: Some(AGENT_ROUTE_AGENTS_REVIEW.to_string()),
            agent: args.agent.clone(),
            prompt: prompt.to_string(),
            instructions: None,
            model: args.model.clone(),
            reasoning: args.reasoning.clone(),
            transport: None,
            attachments: Vec::new(),
        },
    )?;
    Ok(render_invocation_diagnostics(&invocation))
}

fn build_audit_prompt(context: &ReviewContext) -> String {
    let review_comments = render_review_comments(&context.pull_request.comments);
    let reviews = render_reviews(&context.pull_request.reviews);
    let inline_comments = render_inline_comments(&context.inline_comments);
    let changed_files = render_changed_files(&context.pull_request.files);
    let acceptance_criteria = render_bullets(
        &context.acceptance_criteria,
        "_No explicit acceptance-criteria section was found in the Linear description._",
    );
    let broader_warnings = render_bullets(
        &context.repo_context_warnings,
        "_All expected local repo-context inputs are present._",
    );
    let workpad = context
        .active_workpad
        .as_deref()
        .map(|body| truncate_block(body, 12_000))
        .unwrap_or_else(|| {
            "_No active `## Codex Workpad` comment was present on the linked issue._".to_string()
        });

    format!(
        "{AUDIT_PROMPT_ARTIFACT}\n\n## Repository\n- Name: `{repo}`\n- URL: {repo_url}\n\n## Pull Request\n- Number: #{number}\n- Title: {title}\n- URL: {url}\n- Author: `{author}`\n- Head branch: `{head}`\n- Base branch: `{base}`\n- Head SHA: `{sha}`\n- Review decision: `{review_decision}`\n- Changed files: {changed_files_count}\n- Additions: {additions}\n- Deletions: {deletions}\n\n### Pull Request Body\n{body}\n\n### Changed Files\n{changed_files}\n\n### Diff\n```diff\n{diff}\n```\n\n### Top-Level PR Comments\n{review_comments}\n\n### Review Summaries\n{reviews}\n\n### Inline Review Comments\n{inline_comments}\n\n## Linked Linear Issue\n- Identifier: `{issue_identifier}`\n- Title: {issue_title}\n- URL: {issue_url}\n\n### Linear Description\n{issue_description}\n\n### Acceptance Criteria\n{acceptance_criteria}\n\n### Ticket Discussion Context\n{issue_discussion}\n\n### Active Workpad\n{workpad}\n\n### Local Backlog Context\n{local_backlog}\n\n## Repo Context Warnings\n{broader_warnings}\n\n## Workflow Contract\n{workflow_contract}\n\n## Repo Overlay Rules\n{project_rules}\n\n## Codebase Context\n{codebase_context}\n\n## Repo Map\n{repo_map}\n",
        repo = context.repository.name_with_owner,
        repo_url = context.repository.url,
        number = context.pull_request.number,
        title = context.pull_request.title,
        url = context.pull_request.url,
        author = context.pull_request.author.login,
        head = context.pull_request.head_ref_name,
        base = context.pull_request.base_ref_name,
        sha = context.pull_request.head_ref_oid,
        review_decision = context
            .pull_request
            .review_decision
            .as_deref()
            .unwrap_or("unset"),
        changed_files_count = context.pull_request.changed_files,
        additions = context.pull_request.additions,
        deletions = context.pull_request.deletions,
        body = render_optional_text(&context.pull_request.body, "_No PR body was provided._"),
        changed_files = changed_files,
        diff = truncate_block(&context.diff, 40_000),
        review_comments = review_comments,
        reviews = reviews,
        inline_comments = inline_comments,
        issue_identifier = context.issue.identifier,
        issue_title = context.issue.title,
        issue_url = context.issue.url,
        issue_description = render_optional_text(
            context.issue.description.as_deref().unwrap_or_default(),
            "_No Linear description was provided._",
        ),
        acceptance_criteria = acceptance_criteria,
        issue_discussion = render_optional_text(
            &context.issue_discussion,
            "_No discussion context was available._"
        ),
        workpad = workpad,
        local_backlog = context.local_backlog_context,
        broader_warnings = broader_warnings,
        workflow_contract = context.workflow_contract,
        project_rules = context.project_rules,
        codebase_context = context.codebase_context,
        repo_map = context.repo_map,
    )
}

async fn apply_remediation(
    root: &Path,
    args: &AgentsReviewArgs,
    context: &ReviewContext,
    report: &AuditReport,
) -> Result<RemediationOutcome> {
    let gh = GhCli;
    let workspace = prepare_remediation_workspace(root, &context.pull_request)?;
    let remediation_prompt = build_remediation_prompt(&workspace, context, report);
    let agent_summary = run_agent_capture_in_dir(root, &workspace, args, &remediation_prompt)?;
    let status = git_stdout(&workspace, &["status", "--short"])?;
    if status.trim().is_empty() {
        bail!(
            "audit marked pull request #{} as requiring follow-up changes, but the remediation agent left no workspace edits",
            context.pull_request.number
        );
    }

    run_git(&workspace, &["add", "-A"])?;
    let commit_message = format!(
        "meta agents review: remediate PR #{} for {}",
        context.pull_request.number, context.issue.identifier
    );
    run_git(&workspace, &["commit", "-m", commit_message.as_str()])?;
    let commit = git_stdout(&workspace, &["rev-parse", "--short", "HEAD"])?;
    let branch = git_stdout(&workspace, &["branch", "--show-current"])?;
    run_git(&workspace, &["push", "-u", "origin", branch.as_str()])
        .with_context(|| format!("failed to push remediation branch `{branch}`"))?;

    let body_path = workspace.join(".git").join("meta-agents-review-body.md");
    fs::write(
        &body_path,
        render_follow_up_pull_request_body(context, report, &agent_summary),
    )
    .with_context(|| format!("failed to write `{}`", body_path.display()))?;
    let pull_request_url = gh.create_follow_up_pull_request(
        &workspace,
        &branch,
        &context.pull_request.head_ref_name,
        &format!(
            "meta agents review: remediate PR #{} for {}",
            context.pull_request.number, context.issue.identifier
        ),
        &body_path,
    )?;

    let linear_context = load_linear_command_context(&args.client, None)?;
    let comment_body = render_linear_review_comment(context, report, &pull_request_url);
    linear_context
        .service
        .upsert_comment_with_marker(&context.issue, REVIEW_COMMENT_MARKER, comment_body)
        .await
        .with_context(|| {
            format!(
                "created remediation PR `{pull_request_url}` but failed to update the linked Linear issue comment"
            )
        })?;

    Ok(RemediationOutcome {
        branch,
        commit,
        pull_request_url,
        agent_summary: agent_summary.trim().to_string(),
    })
}

fn build_remediation_prompt(
    workspace: &Path,
    context: &ReviewContext,
    report: &AuditReport,
) -> String {
    format!(
        "{REMEDIATION_PROMPT_ARTIFACT}\n\n## Workspace\n- Path: `{}`\n- Base PR: #{} {}\n- Follow-up branch target: `{}`\n- Linked Linear issue: `{}` {}\n\n## Audit Summary\n{}\n\n## Blocking Issues\n{}\n\n## Optional Improvements\n{}\n\n## Suggested Validation\n{}\n\n## Useful Context\n### Acceptance Criteria\n{}\n\n### Local Backlog Context\n{}\n\n### Codebase Context\n{}\n",
        workspace.display(),
        context.pull_request.number,
        context.pull_request.title,
        context.pull_request.head_ref_name,
        context.issue.identifier,
        context.issue.title,
        report.summary,
        render_findings(
            &report.blocking_issues,
            "_No blocking issues were recorded._"
        ),
        render_findings(
            &report.optional_improvements,
            "_No optional improvements were recorded._"
        ),
        render_bullets(
            &report.suggested_validation,
            "_No additional validation was suggested._"
        ),
        render_bullets(
            &context.acceptance_criteria,
            "_No explicit acceptance-criteria section was found in the Linear description._"
        ),
        context.local_backlog_context,
        context.codebase_context,
    )
}

fn prepare_remediation_workspace(root: &Path, pull_request: &GhPullRequestView) -> Result<PathBuf> {
    let workspace_root = sibling_workspace_root(root)?;
    ensure_dir(&workspace_root)?;
    let workspace_path = workspace_root.join(format!("pr-review-{}", pull_request.number));
    if workspace_path.exists() {
        fs::remove_dir_all(&workspace_path)
            .with_context(|| format!("failed to remove `{}`", workspace_path.display()))?;
    }

    let remote_url = git_stdout(root, &["remote", "get-url", "origin"])
        .context("failed to resolve the repository origin remote")?;
    run_git(
        root,
        &[
            "clone",
            "--origin",
            "origin",
            remote_url.as_str(),
            workspace_path
                .to_str()
                .ok_or_else(|| anyhow!("workspace path is not valid utf-8"))?,
        ],
    )?;
    ensure_workspace_path_is_safe(root, &workspace_root, &workspace_path)?;
    configure_workspace_git_identity(root, &workspace_path)?;
    run_git(
        &workspace_path,
        &["fetch", "origin", pull_request.head_ref_name.as_str()],
    )?;
    let branch = remediation_branch_name(pull_request);
    run_git(
        &workspace_path,
        &[
            "checkout",
            "-B",
            branch.as_str(),
            &format!("origin/{}", pull_request.head_ref_name),
        ],
    )?;
    Ok(workspace_path)
}

fn remediation_branch_name(pull_request: &GhPullRequestView) -> String {
    format!(
        "meta-review/pr-{}-{}",
        pull_request.number,
        sanitize_branch_component(&pull_request.head_ref_name)
    )
}

fn sanitize_branch_component(value: &str) -> String {
    let cleaned = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    cleaned
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn configure_workspace_git_identity(source_root: &Path, workspace_path: &Path) -> Result<()> {
    let email = git_config_value(source_root, "user.email")?
        .unwrap_or_else(|| "metastack-cli@example.com".to_string());
    let name =
        git_config_value(source_root, "user.name")?.unwrap_or_else(|| "MetaStack CLI".to_string());
    run_git(workspace_path, &["config", "user.email", email.as_str()])?;
    run_git(workspace_path, &["config", "user.name", name.as_str()])?;
    Ok(())
}

fn git_config_value(root: &Path, key: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["config", "--get", key])
        .output()
        .with_context(|| format!("failed to read git config key `{key}`"))?;
    match output.status.code() {
        Some(0) => Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        )),
        Some(1) => Ok(None),
        _ => bail!(
            "git config --get {key} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    }
}

fn render_dry_run(
    root: &Path,
    pull_request: u64,
    context: &ReviewContext,
    diagnostics: &[String],
    prompt: &str,
) -> String {
    let mut lines = vec![
        format!("Dry run for `meta agents review {pull_request}`"),
        format!("Repository root: `{}`", root.display()),
        format!(
            "Linked Linear issue: `{}` {}",
            context.issue.identifier, context.issue.title
        ),
        format!(
            "Repo context warnings: {}",
            if context.repo_context_warnings.is_empty() {
                "none".to_string()
            } else {
                context.repo_context_warnings.join(" | ")
            }
        ),
        String::new(),
    ];
    lines.extend(diagnostics.iter().cloned());
    lines.extend([
        String::new(),
        "Rendered audit prompt:".to_string(),
        prompt.to_string(),
    ]);
    lines.join("\n")
}

fn render_no_fix_result(
    context: &ReviewContext,
    diagnostics: &[String],
    report: &AuditReport,
) -> String {
    let mut lines = vec![
        format!(
            "Audit completed for PR #{} against `{}`.",
            context.pull_request.number, context.issue.identifier
        ),
        "Required follow-up changes: no".to_string(),
        format!("Summary: {}", report.summary),
        String::new(),
    ];
    lines.extend(diagnostics.iter().cloned());
    if !report.optional_improvements.is_empty() {
        lines.extend([
            String::new(),
            "Optional improvements:".to_string(),
            render_findings(&report.optional_improvements, "_None._"),
        ]);
    }
    lines.join("\n")
}

fn render_remediation_result(
    context: &ReviewContext,
    diagnostics: &[String],
    report: &AuditReport,
    remediation: &RemediationOutcome,
) -> String {
    let mut lines = vec![
        format!(
            "Audit completed for PR #{} against `{}`.",
            context.pull_request.number, context.issue.identifier
        ),
        "Required follow-up changes: yes".to_string(),
        format!("Summary: {}", report.summary),
        format!("Created remediation branch: `{}`", remediation.branch),
        format!("Created remediation commit: `{}`", remediation.commit),
        format!("Created follow-up PR: {}", remediation.pull_request_url),
        String::new(),
    ];
    lines.extend(diagnostics.iter().cloned());
    lines.extend([
        String::new(),
        "Blocking issues:".to_string(),
        render_findings(&report.blocking_issues, "_None._"),
    ]);
    if !remediation.agent_summary.is_empty() {
        lines.extend([
            String::new(),
            "Remediation agent summary:".to_string(),
            remediation.agent_summary.clone(),
        ]);
    }
    lines.join("\n")
}

fn render_follow_up_pull_request_body(
    context: &ReviewContext,
    report: &AuditReport,
    agent_summary: &str,
) -> String {
    format!(
        "# Meta Agents Remediation for PR #{number}\n\n## Context\n\n- Original PR: {original_pr}\n- Linked Linear issue: {issue_url}\n- Original head branch: `{head_branch}`\n- This follow-up PR targets the original PR branch so the fixes can be merged back into the parent change set.\n\n## Why this PR exists\n\n{summary}\n\n## Blocking issues addressed\n\n{blocking_issues}\n\n## Agent remediation summary\n\n{agent_summary}\n",
        number = context.pull_request.number,
        original_pr = context.pull_request.url,
        issue_url = context.issue.url,
        head_branch = context.pull_request.head_ref_name,
        summary = report.summary,
        blocking_issues = render_findings(&report.blocking_issues, "_None recorded._"),
        agent_summary = render_optional_text(
            agent_summary,
            "_The remediation agent did not return a summary._"
        ),
    )
}

fn render_linear_review_comment(
    context: &ReviewContext,
    report: &AuditReport,
    follow_up_pull_request_url: &str,
) -> String {
    format!(
        "{REVIEW_COMMENT_MARKER}\n\nOpened a remediation PR because the audit of GitHub PR #{number} found follow-up work that still blocks `{issue}`.\n\n- Original PR: {original_pr}\n- Remediation PR: {follow_up_pr}\n- Audit summary: {summary}\n\n### Blocking issues\n\n{blocking_issues}\n",
        number = context.pull_request.number,
        issue = context.issue.identifier,
        original_pr = context.pull_request.url,
        follow_up_pr = follow_up_pull_request_url,
        summary = report.summary,
        blocking_issues = render_findings(&report.blocking_issues, "_None recorded._"),
    )
}

fn render_changed_files(files: &[GhChangedFile]) -> String {
    if files.is_empty() {
        return "_No changed files were returned by GitHub._".to_string();
    }

    files
        .iter()
        .map(|file| {
            format!(
                "- `{}` (+{}, -{})",
                file.path, file.additions, file.deletions
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_review_comments(comments: &[GhIssueComment]) -> String {
    if comments.is_empty() {
        return "_No top-level PR comments were returned by GitHub._".to_string();
    }
    comments
        .iter()
        .map(|comment| {
            format!(
                "- `{}`: {}",
                comment.author.login,
                truncate_single_line(&comment.body, 240)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_reviews(reviews: &[GhReview]) -> String {
    if reviews.is_empty() {
        return "_No review summaries were returned by GitHub._".to_string();
    }
    reviews
        .iter()
        .map(|review| {
            format!(
                "- `{}` [{}]: {}",
                review.author.login,
                review.state,
                truncate_single_line(&review.body, 240)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_inline_comments(comments: &[GhInlineReviewComment]) -> String {
    if comments.is_empty() {
        return "_No inline review comments were returned by GitHub._".to_string();
    }
    comments
        .iter()
        .map(|comment| {
            format!(
                "- `{}` on `{}`: {}{}",
                comment.user.login,
                comment.path,
                truncate_single_line(&comment.body, 240),
                comment
                    .diff_hunk
                    .as_deref()
                    .map(|hunk| format!(" | hunk: {}", truncate_single_line(hunk, 120)))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_findings(findings: &[AuditFinding], empty: &str) -> String {
    if findings.is_empty() {
        return empty.to_string();
    }
    findings
        .iter()
        .map(|finding| {
            let mut lines = vec![format!("- {}", finding.title)];
            lines.push(format!("  rationale: {}", finding.rationale));
            if !finding.evidence.is_empty() {
                lines.push(format!("  evidence: {}", finding.evidence.join(" | ")));
            }
            if let Some(suggested_fix) = &finding.suggested_fix {
                lines.push(format!("  suggested fix: {}", suggested_fix));
            }
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_bullets(lines: &[String], empty: &str) -> String {
    if lines.is_empty() {
        return empty.to_string();
    }
    lines
        .iter()
        .map(|line| format!("- {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_optional_text(value: &str, empty: &str) -> String {
    if value.trim().is_empty() {
        empty.to_string()
    } else {
        value.trim().to_string()
    }
}

fn render_local_backlog_context(root: &Path, identifier: &str) -> Result<String> {
    let issue_dir = PlanningPaths::new(root).backlog_issue_dir(identifier);
    if !issue_dir.is_dir() {
        return Ok(format!(
            "_No local backlog packet was found at `{}`._",
            issue_dir.display()
        ));
    }

    let mut sections = Vec::new();
    for relative in [
        "index.md",
        "implementation.md",
        "validation.md",
        "checklist.md",
        "context/ticket-discussion.md",
    ] {
        let path = issue_dir.join(relative);
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => truncate_block(&contents, 8_000),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => "_Missing._".to_string(),
            Err(error) => {
                format!("_Failed to read `{}`: {}_", path.display(), error)
            }
        };
        sections.push(format!("### `{relative}`\n{contents}"));
    }

    Ok(sections.join("\n\n"))
}

fn repo_context_warnings(root: &Path) -> Vec<String> {
    let paths = PlanningPaths::new(root);
    let expected = [
        paths.scan_path(),
        paths.architecture_path(),
        paths.concerns_path(),
        paths.conventions_path(),
        paths.integrations_path(),
        paths.stack_path(),
        paths.structure_path(),
        paths.testing_path(),
    ];
    expected
        .into_iter()
        .filter(|path| !path.is_file())
        .map(|path| {
            format!(
                "missing `{}`; run `meta context reload --root {}` or `meta context scan --root {}`",
                path.display(),
                root.display(),
                root.display()
            )
        })
        .collect()
}

fn extract_acceptance_criteria(description: Option<&str>) -> Vec<String> {
    let Some(description) = description else {
        return Vec::new();
    };

    let mut capture = false;
    let mut lines = Vec::new();
    for line in description.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            let normalized = trimmed.trim_start_matches('#').trim().to_ascii_lowercase();
            if normalized == "acceptance criteria" {
                capture = true;
                continue;
            }
            if capture {
                break;
            }
        }

        if capture && (trimmed.starts_with("- ") || trimmed.starts_with("* ")) {
            lines.push(trimmed[2..].trim().to_string());
        }
    }
    lines
}

fn extract_linear_identifiers(value: &str) -> Vec<String> {
    let mut identifiers = BTreeSet::new();
    for token in value.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-')) {
        if let Some(identifier) = normalize_linear_identifier_token(token) {
            identifiers.insert(identifier);
        }
    }
    identifiers.into_iter().collect()
}

fn normalize_linear_identifier_token(token: &str) -> Option<String> {
    let token = token.trim_matches('-');
    let (prefix, number) = token.split_once('-')?;
    if prefix.is_empty() || number.is_empty() {
        return None;
    }
    if !prefix.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return None;
    }
    if !number.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some(format!("{}-{}", prefix.to_ascii_uppercase(), number))
}

fn truncate_block(value: &str, max_len: usize) -> String {
    if value.chars().count() <= max_len {
        return value.trim().to_string();
    }
    value.chars().take(max_len).collect::<String>() + "\n..."
}

fn truncate_single_line(value: &str, max_len: usize) -> String {
    let collapsed = value.lines().map(str::trim).collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_len {
        collapsed
    } else {
        collapsed.chars().take(max_len).collect::<String>() + "..."
    }
}

fn parse_json_block<T: for<'de> Deserialize<'de>>(value: &str) -> Result<T> {
    if let Ok(parsed) = serde_json::from_str(value) {
        return Ok(parsed);
    }

    let Some(start) = value.find('{') else {
        bail!("agent output did not contain a JSON object");
    };
    let Some(end) = value.rfind('}') else {
        bail!("agent output did not contain a complete JSON object");
    };
    Ok(serde_json::from_str(&value[start..=end])?)
}

fn run_agent_capture_in_dir(
    root: &Path,
    working_dir: &Path,
    args: &AgentsReviewArgs,
    prompt: &str,
) -> Result<String> {
    let config = AppConfig::load()?;
    let planning_meta = PlanningMeta::load(root)?;
    let invocation = resolve_agent_invocation_for_planning(
        &config,
        &planning_meta,
        &RunAgentArgs {
            root: Some(root.to_path_buf()),
            route_key: Some(AGENT_ROUTE_AGENTS_REVIEW.to_string()),
            agent: args.agent.clone(),
            prompt: prompt.to_string(),
            instructions: None,
            model: args.model.clone(),
            reasoning: args.reasoning.clone(),
            transport: None,
            attachments: Vec::new(),
        },
    )?;
    let command_args = command_args_for_invocation(&invocation, Some(working_dir))?;
    let attempted_command = validate_invocation_command_surface(&invocation, &command_args)?;

    let mut command = Command::new(&invocation.command);
    command.args(&command_args);
    command.current_dir(working_dir);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    apply_noninteractive_agent_environment(&mut command);
    apply_invocation_environment(&mut command, &invocation, prompt, None);

    let mut child = command.spawn().with_context(|| {
        format!(
            "failed to launch agent `{}` with command `{attempted_command}`",
            invocation.agent
        )
    })?;
    if invocation.transport == crate::config::PromptTransport::Stdin {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("failed to open stdin for agent `{}`", invocation.agent))?;
        stdin
            .write_all(invocation.payload.as_bytes())
            .with_context(|| {
                format!(
                    "failed to write review prompt payload to agent `{}`",
                    invocation.agent
                )
            })?;
    }

    let output = child
        .wait_with_output()
        .with_context(|| format!("failed to wait for agent `{}`", invocation.agent))?;
    if !output.status.success() {
        bail!(
            "agent `{}` exited unsuccessfully while running `{attempted_command}`: {}",
            invocation.agent,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_git(root: &Path, args: &[&str]) -> Result<()> {
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
    Ok(())
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

impl GhCli {
    fn resolve_repository(&self, root: &Path) -> Result<GhRepositoryView> {
        self.run_json(root, &["repo", "view", "--json", "nameWithOwner,url"])
    }

    fn view_pull_request(&self, root: &Path, number: u64) -> Result<GhPullRequestView> {
        self.run_json(
            root,
            &[
                "pr",
                "view",
                &number.to_string(),
                "--json",
                "number,title,body,url,headRefName,baseRefName,headRefOid,reviewDecision,changedFiles,additions,deletions,author,files,comments,reviews",
            ],
        )
    }

    fn pull_request_diff(&self, root: &Path, number: u64) -> Result<String> {
        self.run_plain_capture(root, &["pr", "diff", &number.to_string()])
    }

    fn pull_request_inline_comments(
        &self,
        root: &Path,
        repository: &str,
        number: u64,
    ) -> Result<Vec<GhInlineReviewComment>> {
        self.run_json(
            root,
            &[
                "api",
                &format!("repos/{repository}/pulls/{number}/comments"),
            ],
        )
    }

    fn create_follow_up_pull_request(
        &self,
        root: &Path,
        head_branch: &str,
        base_branch: &str,
        title: &str,
        body_path: &Path,
    ) -> Result<String> {
        let created: CreatedPullRequest = self.run_json(
            root,
            &[
                "pr",
                "create",
                "--head",
                head_branch,
                "--base",
                base_branch,
                "--title",
                title,
                "--body-file",
                body_path
                    .to_str()
                    .ok_or_else(|| anyhow!("invalid PR body path"))?,
                "--json",
                "url",
            ],
        )?;
        Ok(created.url)
    }

    fn run_json<T: for<'de> Deserialize<'de>>(&self, root: &Path, args: &[&str]) -> Result<T> {
        let output = Command::new("gh")
            .args(args)
            .current_dir(root)
            .output()
            .with_context(|| format!("failed to run `gh {}`", args.join(" ")))?;
        if !output.status.success() {
            bail!(
                "gh {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        serde_json::from_slice(&output.stdout)
            .with_context(|| format!("failed to decode JSON from `gh {}`", args.join(" ")))
    }

    fn run_plain_capture(&self, root: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new("gh")
            .args(args)
            .current_dir(root)
            .output()
            .with_context(|| format!("failed to run `gh {}`", args.join(" ")))?;
        if !output.status.success() {
            bail!(
                "gh {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

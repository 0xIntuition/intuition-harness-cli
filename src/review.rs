use std::collections::BTreeSet;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::agents::{
    apply_invocation_environment, apply_noninteractive_agent_environment,
    command_args_for_invocation, format_agent_config_source, render_invocation_diagnostics,
    resolve_agent_invocation_for_planning, validate_invocation_command_surface,
};
use crate::cli::{LinearClientArgs, ReviewArgs, RunAgentArgs};
use crate::config::{AGENT_ROUTE_AGENTS_REVIEW, AppConfig, PlanningMeta, resolve_data_root};
use crate::context::{load_codebase_context_bundle, load_workflow_contract, render_repo_map};
use crate::fs::{
    canonicalize_existing_dir, ensure_dir, ensure_workspace_path_is_safe, sibling_workspace_root,
};
use crate::linear::{IssueComment, IssueSummary, load_linear_command_context};

const REVIEW_AUDIT_WORKFLOW: &str = include_str!("artifacts/review-audit-workflow.md");
const DEFAULT_REVIEW_POLL_INTERVAL_SECONDS: u64 = 30;
const REVIEW_LABEL: &str = "metastack";

/// Runs `meta agents review` in one-shot PR review mode or listener mode.
///
/// Returns an error when GitHub or Linear context cannot be resolved, when the configured agent
/// launch surface is invalid, or when review/remediation mutations fail.
pub async fn run_review(args: &ReviewArgs) -> Result<()> {
    let root = canonicalize_existing_dir(&args.root)?;
    let _planning_meta = PlanningMeta::load(&root)
        .with_context(|| format!("failed to load planning metadata for `{}`", root.display()))?;
    let gh = GhCli;

    if let Some(number) = args.pull_request {
        let result = review_pull_request(&root, args, &gh, number).await?;
        println!("{}", render_review_result(&result, &root));
        return Ok(());
    }

    run_review_listener(&root, args, &gh).await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GithubRepository {
    name_with_owner: String,
    url: String,
    default_branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GithubActor {
    login: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GithubLabel {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GithubPullRequestListItem {
    number: u64,
    title: String,
    url: String,
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    #[serde(rename = "baseRefName")]
    base_ref_name: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    author: GithubActor,
    #[serde(default)]
    labels: Vec<GithubLabel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GithubReview {
    author: Option<GithubActor>,
    state: Option<String>,
    body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GithubChangedFile {
    path: String,
    additions: i64,
    deletions: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GithubPullRequestDetail {
    number: u64,
    title: String,
    body: String,
    url: String,
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    #[serde(rename = "baseRefName")]
    base_ref_name: String,
    #[serde(rename = "reviewDecision")]
    review_decision: Option<String>,
    #[serde(rename = "changedFiles")]
    changed_files: Option<u64>,
    #[serde(default)]
    files: Vec<GithubChangedFile>,
    #[serde(default)]
    reviews: Vec<GithubReview>,
    author: GithubActor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReviewAuditEntry {
    title: String,
    rationale: String,
    #[serde(default)]
    file_hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReviewAuditResult {
    remediation_required: bool,
    summary: String,
    #[serde(default)]
    required_fixes: Vec<ReviewAuditEntry>,
    #[serde(default)]
    optional_recommendations: Vec<ReviewAuditEntry>,
}

#[derive(Debug, Clone, Serialize)]
struct ReviewAgentResolution {
    provider: String,
    model: Option<String>,
    reasoning: Option<String>,
    route_key: Option<String>,
    family_key: Option<String>,
    provider_source: String,
    model_source: Option<String>,
    reasoning_source: Option<String>,
    diagnostics: Vec<String>,
}

#[derive(Debug, Clone)]
struct ReviewContext {
    repository: GithubRepository,
    pull_request: GithubPullRequestDetail,
    diff: String,
    linear_issue: IssueSummary,
    issue_identifier: String,
    acceptance_criteria: Vec<String>,
    workpad_comment: Option<IssueComment>,
    codebase_context: String,
    repo_map: String,
}

#[derive(Debug, Clone, Serialize)]
struct ReviewExecutionResult {
    pull_request: u64,
    issue_identifier: String,
    remediation_required: bool,
    remediation_pr_url: Option<String>,
    dry_run: bool,
    summary: String,
    required_fixes: Vec<ReviewAuditEntry>,
    optional_recommendations: Vec<ReviewAuditEntry>,
    agent_resolution: ReviewAgentResolution,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReviewSessionPhase {
    Claimed,
    ReviewStarted,
    Running,
    Completed,
    Blocked,
    Retryable,
}

impl ReviewSessionPhase {
    fn display_label(&self) -> &'static str {
        match self {
            Self::Claimed => "Claimed",
            Self::ReviewStarted => "Review Started",
            Self::Running => "Running",
            Self::Completed => "Completed",
            Self::Blocked => "Blocked",
            Self::Retryable => "Retryable",
        }
    }

    fn is_active(&self) -> bool {
        matches!(self, Self::Claimed | Self::ReviewStarted | Self::Running)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReviewSession {
    pull_request: u64,
    title: String,
    url: String,
    head_ref: String,
    phase: ReviewSessionPhase,
    summary: String,
    updated_at_epoch_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ReviewState {
    version: u8,
    sessions: Vec<ReviewSession>,
}

impl ReviewState {
    fn upsert(&mut self, session: ReviewSession) {
        if let Some(existing) = self
            .sessions
            .iter_mut()
            .find(|existing| existing.pull_request == session.pull_request)
        {
            *existing = session;
        } else {
            self.sessions.push(session);
        }
        self.sessions.sort_by(|left, right| {
            right
                .updated_at_epoch_seconds
                .cmp(&left.updated_at_epoch_seconds)
                .then_with(|| left.pull_request.cmp(&right.pull_request))
        });
    }

    fn blocks_pickup(&self, pull_request: u64) -> bool {
        self.sessions
            .iter()
            .any(|session| session.pull_request == pull_request && session.phase.is_active())
    }
}

#[derive(Debug, Clone, Serialize)]
struct ReviewDiscovery {
    repository: GithubRepository,
    pull_requests: Vec<GithubPullRequestListItem>,
    sessions: Vec<ReviewSession>,
}

#[derive(Debug)]
struct ReviewStore {
    project_dir: PathBuf,
    state_path: PathBuf,
    lock_path: PathBuf,
}

impl ReviewStore {
    fn resolve(root: &Path, repository: &GithubRepository) -> Result<Self> {
        let data_root = resolve_data_root()?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        root.display().to_string().hash(&mut hasher);
        repository.name_with_owner.hash(&mut hasher);
        let key = format!("{:016x}", hasher.finish());
        let project_dir = data_root.join("review").join("projects").join(key);
        Ok(Self {
            state_path: project_dir.join("session.json"),
            lock_path: project_dir.join("active-review.lock.json"),
            project_dir,
        })
    }

    fn ensure_layout(&self) -> Result<()> {
        ensure_dir(&self.project_dir)?;
        Ok(())
    }

    fn load_state(&self) -> Result<ReviewState> {
        if !self.state_path.exists() {
            return Ok(ReviewState {
                version: 1,
                sessions: Vec::new(),
            });
        }
        read_json(&self.state_path)
    }

    fn save_state(&self, state: &ReviewState) -> Result<()> {
        self.ensure_layout()?;
        write_json(&self.state_path, state)
    }

    fn upsert_session(&self, session: ReviewSession) -> Result<()> {
        let mut state = self.load_state()?;
        state.upsert(session);
        self.save_state(&state)
    }

    fn acquire_lock(&self, pid: u32) -> Result<ReviewLockGuard> {
        self.ensure_layout()?;
        if self.lock_path.exists() {
            let existing: ReviewLock = read_json(&self.lock_path)?;
            if pid_is_running(existing.pid) {
                bail!(
                    "another `meta agents review` listener already owns this repository (pid {}); active lock: {}",
                    existing.pid,
                    self.lock_path.display()
                );
            }
            let _ = fs::remove_file(&self.lock_path);
        }

        write_json(
            &self.lock_path,
            &ReviewLock {
                pid,
                acquired_at_epoch_seconds: now_epoch_seconds(),
            },
        )?;
        Ok(ReviewLockGuard {
            lock_path: self.lock_path.clone(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReviewLock {
    pid: u32,
    acquired_at_epoch_seconds: u64,
}

#[derive(Debug)]
struct ReviewLockGuard {
    lock_path: PathBuf,
}

impl Drop for ReviewLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock_path);
    }
}

#[derive(Debug, Clone)]
struct GhCli;

impl GhCli {
    fn resolve_repository(&self, root: &Path) -> Result<GithubRepository> {
        let response: RepoViewResponse = self.read_json(
            root,
            &[
                "repo",
                "view",
                "--json",
                "nameWithOwner,url,defaultBranchRef",
            ],
        )?;
        Ok(GithubRepository {
            name_with_owner: response.name_with_owner,
            url: response.url,
            default_branch: response.default_branch_ref.name,
        })
    }

    fn list_labeled_pull_requests(&self, root: &Path) -> Result<Vec<GithubPullRequestListItem>> {
        self.read_json(
            root,
            &[
                "pr",
                "list",
                "--state",
                "open",
                "--label",
                REVIEW_LABEL,
                "--json",
                "number,title,url,headRefName,baseRefName,updatedAt,author,labels",
            ],
        )
    }

    fn pull_request_detail(&self, root: &Path, number: u64) -> Result<GithubPullRequestDetail> {
        self.read_json(
            root,
            &[
                "pr",
                "view",
                &number.to_string(),
                "--json",
                "number,title,body,url,headRefName,baseRefName,reviewDecision,changedFiles,files,reviews,author",
            ],
        )
    }

    fn pull_request_diff(&self, root: &Path, number: u64) -> Result<String> {
        self.stdout(root, &["pr", "diff", &number.to_string()])
            .with_context(|| format!("failed to load diff for pull request #{number}"))
    }

    fn find_existing_follow_up_pr(&self, root: &Path, head_ref: &str) -> Result<Option<String>> {
        let prs: Vec<ExistingPullRequest> = self.read_json(
            root,
            &[
                "pr", "list", "--state", "open", "--head", head_ref, "--json", "url",
            ],
        )?;
        Ok(prs.into_iter().next().map(|pr| pr.url))
    }

    fn create_follow_up_pr(
        &self,
        root: &Path,
        base_ref: &str,
        head_ref: &str,
        title: &str,
        body: &str,
    ) -> Result<String> {
        let created: CreatedPullRequest = self.read_json(
            root,
            &[
                "pr", "create", "--base", base_ref, "--head", head_ref, "--title", title, "--body",
                body,
            ],
        )?;
        Ok(created.url)
    }

    fn read_json<T>(&self, root: &Path, args: &[&str]) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let stdout = self.stdout(root, args)?;
        serde_json::from_str(&stdout).with_context(|| {
            format!(
                "failed to decode GitHub CLI JSON from `gh {}`",
                args.join(" ")
            )
        })
    }

    fn stdout(&self, root: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new("gh")
            .arg("-R")
            .arg(resolve_origin_repo_slug(root)?)
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
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct RepoViewResponse {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
    url: String,
    #[serde(rename = "defaultBranchRef")]
    default_branch_ref: GithubBranchRef,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubBranchRef {
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ExistingPullRequest {
    url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CreatedPullRequest {
    url: String,
}

async fn run_review_listener(root: &Path, args: &ReviewArgs, gh: &GhCli) -> Result<()> {
    let repository = gh.resolve_repository(root)?;
    let store = ReviewStore::resolve(root, &repository)?;

    if args.check {
        let resolution = resolve_review_agent_resolution(root, args, "review preflight")?;
        println!(
            "{}",
            render_preflight_report(root, &repository, args, &resolution)
        );
        return Ok(());
    }

    let _lock = store.acquire_lock(std::process::id())?;
    let state = store.load_state()?;
    let pull_requests = gh.list_labeled_pull_requests(root)?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ReviewDiscovery {
                repository,
                pull_requests,
                sessions: state.sessions,
            })?
        );
        return Ok(());
    }

    if args.render_once {
        println!(
            "{}",
            render_review_dashboard(&repository, &pull_requests, &state.sessions, args)
        );
        return Ok(());
    }

    if args.once {
        let summary =
            process_review_cycle(root, args, gh, &repository, &store, &pull_requests).await?;
        println!("{summary}");
        return Ok(());
    }

    loop {
        let pull_requests = gh.list_labeled_pull_requests(root)?;
        let summary =
            process_review_cycle(root, args, gh, &repository, &store, &pull_requests).await?;
        println!("{summary}");
        thread::sleep(Duration::from_secs(
            args.poll_interval
                .unwrap_or(DEFAULT_REVIEW_POLL_INTERVAL_SECONDS),
        ));
    }
}

async fn process_review_cycle(
    root: &Path,
    args: &ReviewArgs,
    gh: &GhCli,
    repository: &GithubRepository,
    store: &ReviewStore,
    pull_requests: &[GithubPullRequestListItem],
) -> Result<String> {
    let mut state = store.load_state()?;
    let mut reviewed = 0usize;
    for pr in pull_requests {
        if state.blocks_pickup(pr.number) {
            continue;
        }
        reviewed += 1;
        store.upsert_session(ReviewSession {
            pull_request: pr.number,
            title: pr.title.clone(),
            url: pr.url.clone(),
            head_ref: pr.head_ref_name.clone(),
            phase: ReviewSessionPhase::Claimed,
            summary: "Claimed for review.".to_string(),
            updated_at_epoch_seconds: now_epoch_seconds(),
        })?;
        store.upsert_session(ReviewSession {
            pull_request: pr.number,
            title: pr.title.clone(),
            url: pr.url.clone(),
            head_ref: pr.head_ref_name.clone(),
            phase: ReviewSessionPhase::ReviewStarted,
            summary: "Review context assembly started.".to_string(),
            updated_at_epoch_seconds: now_epoch_seconds(),
        })?;
        store.upsert_session(ReviewSession {
            pull_request: pr.number,
            title: pr.title.clone(),
            url: pr.url.clone(),
            head_ref: pr.head_ref_name.clone(),
            phase: ReviewSessionPhase::Running,
            summary: "Running one-shot review/remediation workflow.".to_string(),
            updated_at_epoch_seconds: now_epoch_seconds(),
        })?;

        match review_pull_request(root, args, gh, pr.number).await {
            Ok(result) => {
                store.upsert_session(ReviewSession {
                    pull_request: pr.number,
                    title: pr.title.clone(),
                    url: pr.url.clone(),
                    head_ref: pr.head_ref_name.clone(),
                    phase: ReviewSessionPhase::Completed,
                    summary: if result.remediation_required {
                        format!(
                            "Completed | remediation opened: {}",
                            result
                                .remediation_pr_url
                                .as_deref()
                                .unwrap_or("url unavailable")
                        )
                    } else {
                        "Completed | no remediation required".to_string()
                    },
                    updated_at_epoch_seconds: now_epoch_seconds(),
                })?;
            }
            Err(error) => {
                store.upsert_session(ReviewSession {
                    pull_request: pr.number,
                    title: pr.title.clone(),
                    url: pr.url.clone(),
                    head_ref: pr.head_ref_name.clone(),
                    phase: ReviewSessionPhase::Retryable,
                    summary: format!("Retryable | {error:#}"),
                    updated_at_epoch_seconds: now_epoch_seconds(),
                })?;
            }
        }
        state = store.load_state()?;
    }

    Ok(format!(
        "meta agents review ({}) watched {} labeled PR(s); launched {} review run(s).",
        repository.name_with_owner,
        pull_requests.len(),
        reviewed
    ))
}

async fn review_pull_request(
    root: &Path,
    args: &ReviewArgs,
    gh: &GhCli,
    number: u64,
) -> Result<ReviewExecutionResult> {
    let context = assemble_review_context(root, args, gh, number).await?;
    let prompt = build_review_prompt(root, &context)?;
    let resolution = resolve_review_agent_resolution(root, args, &prompt)?;

    if args.dry_run {
        return Ok(ReviewExecutionResult {
            pull_request: number,
            issue_identifier: context.issue_identifier,
            remediation_required: false,
            remediation_pr_url: None,
            dry_run: true,
            summary: format!(
                "Dry run only. Planned review for PR #{} against linked Linear issue {}.",
                number, context.linear_issue.identifier
            ),
            required_fixes: Vec::new(),
            optional_recommendations: Vec::new(),
            agent_resolution: resolution,
        });
    }

    let audit = run_review_audit_agent(root, args, &prompt)?;
    if !audit.remediation_required {
        return Ok(ReviewExecutionResult {
            pull_request: number,
            issue_identifier: context.issue_identifier,
            remediation_required: false,
            remediation_pr_url: None,
            dry_run: false,
            summary: audit.summary,
            required_fixes: audit.required_fixes,
            optional_recommendations: audit.optional_recommendations,
            agent_resolution: resolution,
        });
    }

    let remediation_pr_url = apply_review_remediation(root, args, gh, &context, &audit).await?;

    Ok(ReviewExecutionResult {
        pull_request: number,
        issue_identifier: context.issue_identifier,
        remediation_required: true,
        remediation_pr_url: Some(remediation_pr_url),
        dry_run: false,
        summary: audit.summary,
        required_fixes: audit.required_fixes,
        optional_recommendations: audit.optional_recommendations,
        agent_resolution: resolution,
    })
}

async fn assemble_review_context(
    root: &Path,
    args: &ReviewArgs,
    gh: &GhCli,
    number: u64,
) -> Result<ReviewContext> {
    let repository = gh.resolve_repository(root)?;
    let pull_request = gh.pull_request_detail(root, number)?;
    let diff = gh.pull_request_diff(root, number)?;
    let issue_identifier = resolve_linked_linear_issue(&pull_request)?;
    let linear = load_linear_command_context(
        &LinearClientArgs {
            api_key: args.api_key.clone(),
            api_url: args.api_url.clone(),
            profile: args.profile.clone(),
            root: root.to_path_buf(),
        },
        args.team.clone(),
    )?;
    let linear_issue = linear
        .service
        .load_issue(&issue_identifier)
        .await
        .with_context(|| format!("failed to load linked Linear issue `{issue_identifier}`"))?;
    let acceptance_criteria = extract_acceptance_criteria(linear_issue.description.as_deref());
    let workpad_comment = linear_issue
        .comments
        .iter()
        .find(|comment| comment.body.contains("## Codex Workpad"))
        .cloned();

    Ok(ReviewContext {
        repository,
        pull_request,
        diff,
        issue_identifier,
        acceptance_criteria,
        workpad_comment,
        codebase_context: load_codebase_context_bundle(root)?,
        repo_map: render_repo_map(root)?,
        linear_issue,
    })
}

fn build_review_prompt(root: &Path, context: &ReviewContext) -> Result<String> {
    let workpad_body = context
        .workpad_comment
        .as_ref()
        .map(|comment| comment.body.as_str())
        .unwrap_or("_No active workpad comment was found._");
    let acceptance = if context.acceptance_criteria.is_empty() {
        "_No explicit acceptance criteria were parsed from the Linear issue._".to_string()
    } else {
        context
            .acceptance_criteria
            .iter()
            .map(|criterion| format!("- {criterion}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let files = if context.pull_request.files.is_empty() {
        "_No changed-file metadata was returned by GitHub._".to_string()
    } else {
        context
            .pull_request
            .files
            .iter()
            .map(|file| format!("- {} (+{}, -{})", file.path, file.additions, file.deletions))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let reviews = if context.pull_request.reviews.is_empty() {
        "_No prior review entries were returned by GitHub._".to_string()
    } else {
        context
            .pull_request
            .reviews
            .iter()
            .map(|review| {
                format!(
                    "- {} [{}] {}",
                    review
                        .author
                        .as_ref()
                        .map(|author| author.login.as_str())
                        .unwrap_or("unknown"),
                    review.state.as_deref().unwrap_or("unknown"),
                    review.body.as_deref().unwrap_or("").trim()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    Ok(format!(
        "{workflow}\n\nRepository root: {root}\nRepository: {repo}\nRepository URL: {repo_url}\n\nPull request:\n- Number: #{number}\n- Title: {title}\n- URL: {url}\n- Author: {author}\n- Base: {base}\n- Head: {head}\n- Review decision: {review_decision}\n- Changed files reported: {changed_files}\n\nChanged files:\n{files}\n\nGitHub review state:\n{reviews}\n\nLinked Linear issue: {issue_identifier}\nLinear title: {linear_title}\nLinear URL: {linear_url}\n\nAcceptance Criteria:\n{acceptance}\n\nWorkpad/comment context:\n{workpad_body}\n\nInjected workflow contract:\n{workflow_contract}\n\nRepo map:\n{repo_map}\n\nCodebase context:\n{codebase_context}\n\nPull request diff:\n{diff}\n",
        workflow = REVIEW_AUDIT_WORKFLOW.trim(),
        root = root.display(),
        repo = context.repository.name_with_owner,
        repo_url = context.repository.url,
        number = context.pull_request.number,
        title = context.pull_request.title,
        url = context.pull_request.url,
        author = context.pull_request.author.login,
        base = context.pull_request.base_ref_name,
        head = context.pull_request.head_ref_name,
        review_decision = context
            .pull_request
            .review_decision
            .as_deref()
            .unwrap_or("unset"),
        changed_files = context.pull_request.changed_files.unwrap_or(0),
        files = files,
        reviews = reviews,
        issue_identifier = context.issue_identifier,
        linear_title = context.linear_issue.title,
        linear_url = context.linear_issue.url,
        acceptance = acceptance,
        workpad_body = workpad_body,
        workflow_contract = load_workflow_contract(root)?,
        repo_map = context.repo_map,
        codebase_context = context.codebase_context,
        diff = context.diff,
    ))
}

fn resolve_review_agent_resolution(
    root: &Path,
    args: &ReviewArgs,
    prompt: &str,
) -> Result<ReviewAgentResolution> {
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
    let diagnostics = render_invocation_diagnostics(&invocation);

    Ok(ReviewAgentResolution {
        provider: invocation.agent,
        model: invocation.model,
        reasoning: invocation.reasoning,
        route_key: invocation.route_key,
        family_key: invocation.family_key,
        provider_source: format_agent_config_source(&invocation.provider_source),
        model_source: invocation
            .model_source
            .map(|source| format_agent_config_source(&source)),
        reasoning_source: invocation
            .reasoning_source
            .map(|source| format_agent_config_source(&source)),
        diagnostics,
    })
}

fn run_review_audit_agent(
    root: &Path,
    args: &ReviewArgs,
    prompt: &str,
) -> Result<ReviewAuditResult> {
    let output = run_agent_capture_in_dir(
        root,
        root,
        args,
        prompt,
        "failed to produce the review audit",
    )?;
    extract_json_object(&output)
}

async fn apply_review_remediation(
    root: &Path,
    args: &ReviewArgs,
    gh: &GhCli,
    context: &ReviewContext,
    audit: &ReviewAuditResult,
) -> Result<String> {
    let workspace_root = sibling_workspace_root(root)?;
    ensure_dir(&workspace_root)?;
    let run_id = format!("pr-{}-{}", context.pull_request.number, now_epoch_seconds());
    let workspace_path = workspace_root.join(&run_id);
    prepare_review_workspace(
        root,
        &workspace_root,
        &workspace_path,
        &context.pull_request.head_ref_name,
        &run_id,
    )?;

    let remediation_prompt = build_remediation_prompt(context, audit);
    let _ = run_agent_capture_in_dir(
        root,
        &workspace_path,
        args,
        &remediation_prompt,
        "failed to apply remediation edits",
    )?;
    let status = git_stdout(&workspace_path, &["status", "--short"])?;
    if status.trim().is_empty() {
        bail!(
            "remediation was required for PR #{}, but the agent did not change any files",
            context.pull_request.number
        );
    }

    let remediation_branch = format!(
        "meta-review/pr-{}-{}",
        context.pull_request.number,
        now_epoch_seconds()
    );
    run_git(&workspace_path, &["checkout", "-B", &remediation_branch])?;
    run_git(&workspace_path, &["add", "-A"])?;
    run_git(
        &workspace_path,
        &[
            "commit",
            "-m",
            &format!(
                "meta agents review: remediate PR #{}",
                context.pull_request.number
            ),
        ],
    )?;
    run_git(
        &workspace_path,
        &[
            "push",
            "-u",
            "origin",
            &format!("HEAD:{remediation_branch}"),
        ],
    )
    .context("git push failed while publishing the remediation branch")?;

    let title = format!(
        "meta agents review remediation for PR #{}: {}",
        context.pull_request.number, context.pull_request.title
    );
    let body = format!(
        "Opened automatically by `meta agents review`.\n\nOriginal PR: {}\nLinked Linear issue: {}\n\nRequired fixes:\n{}\n",
        context.pull_request.url,
        context.linear_issue.identifier,
        audit
            .required_fixes
            .iter()
            .map(|fix| format!("- {}: {}", fix.title, fix.rationale))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let remediation_pr_url =
        match gh.find_existing_follow_up_pr(&workspace_path, &remediation_branch)? {
            Some(url) => url,
            None => gh
                .create_follow_up_pr(
                    &workspace_path,
                    &context.pull_request.head_ref_name,
                    &remediation_branch,
                    &title,
                    &body,
                )
                .context("GitHub remediation PR creation failed")?,
        };

    let linear = load_linear_command_context(
        &LinearClientArgs {
            api_key: args.api_key.clone(),
            api_url: args.api_url.clone(),
            profile: args.profile.clone(),
            root: root.to_path_buf(),
        },
        args.team.clone(),
    )?;
    linear
        .service
        .create_issue_comment(
            &context.linear_issue,
            format!(
                "Opened remediation PR {} for GitHub PR #{} because `meta agents review` found blocking fixes.\n\nSummary: {}",
                remediation_pr_url, context.pull_request.number, audit.summary
            ),
        )
        .await
        .context("Linear comment failure while reporting the remediation PR")?;

    Ok(remediation_pr_url)
}

fn build_remediation_prompt(context: &ReviewContext, audit: &ReviewAuditResult) -> String {
    format!(
        "You are applying required fixes for GitHub PR #{} in the current workspace.\n\nOriginal PR title: {}\nOriginal PR URL: {}\nLinked Linear issue: {}\n\nRequired fixes:\n{}\n\nRules:\n- Edit the workspace directly.\n- Keep changes narrowly scoped to the required fixes.\n- Do not create a summary JSON response.\n- Stop after the code changes are applied.\n",
        context.pull_request.number,
        context.pull_request.title,
        context.pull_request.url,
        context.linear_issue.identifier,
        audit
            .required_fixes
            .iter()
            .map(|fix| format!("- {}: {}", fix.title, fix.rationale))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn render_review_result(result: &ReviewExecutionResult, root: &Path) -> String {
    let mut lines = vec![
        format!(
            "meta agents review completed for PR #{} ({})",
            result.pull_request, result.issue_identifier
        ),
        result.summary.clone(),
    ];
    if result.dry_run {
        lines.push("Dry run: no GitHub or Linear mutations were applied.".to_string());
    } else if result.remediation_required {
        lines.push(format!(
            "Remediation required: yes ({})",
            result
                .remediation_pr_url
                .as_deref()
                .unwrap_or("remediation PR unavailable")
        ));
    } else {
        lines.push("Remediation required: no".to_string());
    }

    if !result.required_fixes.is_empty() {
        lines.push("Required fixes:".to_string());
        for fix in &result.required_fixes {
            lines.push(format!("- {}: {}", fix.title, fix.rationale));
        }
    }
    if !result.optional_recommendations.is_empty() {
        lines.push("Optional recommendations:".to_string());
        for recommendation in &result.optional_recommendations {
            lines.push(format!(
                "- {}: {}",
                recommendation.title, recommendation.rationale
            ));
        }
    }
    lines.push(format!("Repository root: `{}`", root.display()));
    lines.extend(result.agent_resolution.diagnostics.clone());
    lines.join("\n")
}

fn render_preflight_report(
    root: &Path,
    repository: &GithubRepository,
    args: &ReviewArgs,
    resolution: &ReviewAgentResolution,
) -> String {
    let mut lines = vec![
        format!(
            "meta agents review preflight passed for `{}`.",
            repository.name_with_owner
        ),
        format!("Repository root: `{}`", root.display()),
        format!(
            "Mode: {}",
            if args.pull_request.is_some() {
                "one-shot review"
            } else {
                "listener"
            }
        ),
    ];
    lines.extend(resolution.diagnostics.clone());
    lines.join("\n")
}

fn render_review_dashboard(
    repository: &GithubRepository,
    pull_requests: &[GithubPullRequestListItem],
    sessions: &[ReviewSession],
    args: &ReviewArgs,
) -> String {
    let mut lines = vec![
        format!("meta agents review ({})", repository.name_with_owner),
        format!(
            "GitHub refresh cadence: {}s",
            args.poll_interval
                .unwrap_or(DEFAULT_REVIEW_POLL_INTERVAL_SECONDS)
        ),
        format!("Watching label: {REVIEW_LABEL}"),
    ];

    if pull_requests.is_empty() {
        lines.push(
            "No open pull requests with the `metastack` label are waiting for review.".to_string(),
        );
    } else {
        lines.push("Open labeled pull requests:".to_string());
        for pr in pull_requests {
            lines.push(format!(
                "- #{} {} [{}]",
                pr.number, pr.title, pr.head_ref_name
            ));
        }
    }

    if sessions.is_empty() {
        lines.push("Stored review sessions: none".to_string());
    } else {
        lines.push("Stored review sessions:".to_string());
        for session in sessions {
            lines.push(format!(
                "- #{} {} | {} | {}",
                session.pull_request,
                session.title,
                session.phase.display_label(),
                session.summary
            ));
        }
    }

    lines.join("\n")
}

fn resolve_linked_linear_issue(pull_request: &GithubPullRequestDetail) -> Result<String> {
    let mut matches = BTreeSet::new();
    for source in [
        pull_request.title.as_str(),
        pull_request.body.as_str(),
        pull_request.head_ref_name.as_str(),
        pull_request.base_ref_name.as_str(),
    ] {
        matches.extend(extract_issue_identifiers(source));
    }
    match matches.len() {
        0 => bail!(
            "failed to resolve a linked Linear issue from PR #{}; include exactly one issue identifier such as `ENG-1234` in the title, body, or branch name",
            pull_request.number
        ),
        1 => Ok(matches.into_iter().next().expect("single match")),
        _ => bail!(
            "failed to resolve a linked Linear issue from PR #{} because multiple identifiers were found: {}",
            pull_request.number,
            matches.into_iter().collect::<Vec<_>>().join(", ")
        ),
    }
}

fn extract_issue_identifiers(input: &str) -> Vec<String> {
    let mut matches = BTreeSet::new();
    for token in input.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-')) {
        let mut parts = token.split('-');
        let Some(prefix) = parts.next() else {
            continue;
        };
        let Some(number) = parts.next() else {
            continue;
        };
        if parts.next().is_some() {
            continue;
        }
        if prefix.len() < 2
            || !prefix
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
            || !number.chars().all(|ch| ch.is_ascii_digit())
        {
            continue;
        }
        matches.insert(format!("{prefix}-{number}"));
    }
    matches.into_iter().collect()
}

fn extract_acceptance_criteria(description: Option<&str>) -> Vec<String> {
    let Some(description) = description else {
        return Vec::new();
    };
    let mut in_section = false;
    let mut criteria = Vec::new();
    for line in description.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            let heading = trimmed.trim_start_matches('#').trim();
            in_section = heading.eq_ignore_ascii_case("Acceptance Criteria");
            continue;
        }
        if in_section {
            if let Some(value) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            {
                criteria.push(value.trim().to_string());
                continue;
            }
            if trimmed.is_empty() && !criteria.is_empty() {
                break;
            }
        }
    }
    criteria
}

fn extract_json_object<T>(value: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let Some(start) = value.find('{') else {
        bail!("agent output did not contain a JSON object");
    };
    let Some(end) = value.rfind('}') else {
        bail!("agent output did not contain a complete JSON object");
    };
    serde_json::from_str(&value[start..=end]).context("failed to decode agent JSON output")
}

fn run_agent_capture_in_dir(
    root: &Path,
    working_dir: &Path,
    args: &ReviewArgs,
    prompt: &str,
    failure_context: &str,
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
    let attempted = validate_invocation_command_surface(&invocation, &command_args)?;

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
            "failed to launch agent `{}` with command `{attempted}`",
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
                    "failed to write prompt payload for agent `{}`",
                    invocation.agent
                )
            })?;
    }

    let output = child
        .wait_with_output()
        .with_context(|| format!("failed while waiting for agent `{}`", invocation.agent))?;
    if !output.status.success() {
        bail!(
            "{}: agent `{}` exited unsuccessfully: {}",
            failure_context,
            invocation.agent,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn prepare_review_workspace(
    root: &Path,
    workspace_root: &Path,
    workspace_path: &Path,
    head_ref: &str,
    branch_suffix: &str,
) -> Result<()> {
    let raw_remote_url = git_stdout(root, &["config", "--get", "remote.origin.url"])
        .or_else(|_| git_stdout(root, &["remote", "get-url", "origin"]))
        .context("failed to resolve the repository origin remote")?;
    let remote_url = git_stdout(root, &["remote", "get-url", "origin"])
        .context("failed to resolve the repository origin remote")?;
    run_git(root, &["fetch", "origin", head_ref])?;
    run_git(
        root,
        &[
            "clone",
            "--origin",
            "origin",
            &remote_url,
            workspace_path
                .to_str()
                .ok_or_else(|| anyhow!("workspace path is not valid utf-8"))?,
        ],
    )?;
    ensure_workspace_path_is_safe(root, workspace_root, workspace_path)?;
    configure_review_workspace_git_identity(root, workspace_path)?;
    run_git(
        workspace_path,
        &["remote", "set-url", "origin", &raw_remote_url],
    )?;
    run_git(
        workspace_path,
        &[
            "checkout",
            "-B",
            &format!("meta-review-base-{branch_suffix}"),
            &format!("origin/{head_ref}"),
        ],
    )?;
    Ok(())
}

fn configure_review_workspace_git_identity(
    source_root: &Path,
    workspace_path: &Path,
) -> Result<()> {
    let email = git_config_value(source_root, "user.email")?
        .unwrap_or_else(|| "metastack-cli@example.com".to_string());
    let name =
        git_config_value(source_root, "user.name")?.unwrap_or_else(|| "MetaStack CLI".to_string());

    run_git(workspace_path, &["config", "user.email", email.as_str()])?;
    run_git(workspace_path, &["config", "user.name", name.as_str()])?;
    Ok(())
}

fn resolve_origin_repo_slug(root: &Path) -> Result<String> {
    let remote = git_stdout(root, &["config", "--get", "remote.origin.url"])
        .or_else(|_| git_stdout(root, &["remote", "get-url", "origin"]))
        .context("failed to resolve the repository origin remote")?;
    parse_repo_slug(&remote).ok_or_else(|| {
        anyhow!(
            "failed to derive the GitHub repository from origin `{remote}`; use a standard GitHub HTTPS or SSH remote"
        )
    })
}

fn parse_repo_slug(remote: &str) -> Option<String> {
    let trimmed = remote.trim_end_matches(".git");
    if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        return Some(rest.to_string());
    }
    if let Some(index) = trimmed.find("github.com/") {
        return Some(trimmed[index + "github.com/".len()..].to_string());
    }
    None
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

fn write_json<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(value)
            .with_context(|| format!("failed to serialize `{}`", path.display()))?,
    )
    .with_context(|| format!("failed to write `{}`", path.display()))
}

fn read_json<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read `{}`", path.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("failed to decode `{}`", path.display()))
}

fn now_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn pid_is_running(pid: u32) -> bool {
    Command::new("ps")
        .arg("-p")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::{extract_acceptance_criteria, extract_issue_identifiers, parse_repo_slug};

    #[test]
    fn extract_issue_identifiers_finds_distinct_linear_keys() {
        assert_eq!(
            extract_issue_identifiers("ENG-123 fix for PR and META-9 follow-up"),
            vec!["ENG-123".to_string(), "META-9".to_string()]
        );
    }

    #[test]
    fn extract_acceptance_criteria_reads_markdown_section() {
        let criteria = extract_acceptance_criteria(Some(
            "# Title\n\n## Acceptance Criteria\n- First\n- Second\n\n## Validation\n- cargo test\n",
        ));
        assert_eq!(criteria, vec!["First".to_string(), "Second".to_string()]);
    }

    #[test]
    fn parse_repo_slug_supports_https_and_ssh() {
        assert_eq!(
            parse_repo_slug("git@github.com:metastack-systems/metastack-cli.git"),
            Some("metastack-systems/metastack-cli".to_string())
        );
        assert_eq!(
            parse_repo_slug("https://github.com/metastack-systems/metastack-cli.git"),
            Some("metastack-systems/metastack-cli".to_string())
        );
    }
}

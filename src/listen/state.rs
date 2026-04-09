use serde::{Deserialize, Serialize};

use crate::linear::{AttachmentSummary, IssueSummary, LinearFailureKind};

use super::{compact_identifier, format_duration, format_number};

pub(super) const COMPLETED_SESSION_TTL_SECONDS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinearFailureSnapshot {
    pub kind: LinearFailureKind,
    pub message: String,
    pub observed_at_epoch_seconds: u64,
    #[serde(default)]
    pub status_code: Option<u16>,
    #[serde(default)]
    pub consecutive_failures: u32,
    #[serde(default)]
    pub next_retry_at_epoch_seconds: Option<u64>,
}

impl LinearFailureSnapshot {
    pub(super) fn retry_label(&self, now_epoch_seconds: u64) -> String {
        match self.next_retry_at_epoch_seconds {
            Some(next_retry) if next_retry > now_epoch_seconds => {
                format_duration(next_retry.saturating_sub(now_epoch_seconds))
            }
            Some(_) => "now".to_string(),
            None => "manual".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingPullRequestAttachment {
    pub number: u64,
    pub url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingLinearSync {
    #[serde(default)]
    pub require_issue_refresh: bool,
    #[serde(default)]
    pub workpad_body: Option<String>,
    #[serde(default)]
    pub pull_request_attachment: Option<PendingPullRequestAttachment>,
    #[serde(default)]
    pub review_transition_issue: bool,
    #[serde(default)]
    pub review_transition_backlog_issue: Option<String>,
    #[serde(default)]
    pub last_failure: Option<LinearFailureSnapshot>,
}

impl PendingLinearSync {
    pub(super) fn is_empty(&self) -> bool {
        !self.require_issue_refresh
            && self.workpad_body.is_none()
            && self.pull_request_attachment.is_none()
            && !self.review_transition_issue
            && self.review_transition_backlog_issue.is_none()
    }

    pub(super) fn blocks_agent_turns(&self) -> bool {
        self.require_issue_refresh
            || self.review_transition_issue
            || self.review_transition_backlog_issue.is_some()
    }

    pub(super) fn operation_labels(&self) -> Vec<String> {
        let mut labels = Vec::new();
        if self.require_issue_refresh {
            labels.push("issue refresh".to_string());
        }
        if self.workpad_body.is_some() {
            labels.push("workpad sync".to_string());
        }
        if self.pull_request_attachment.is_some() {
            labels.push("PR attachment".to_string());
        }
        if self.review_transition_issue {
            labels.push("issue review transition".to_string());
        }
        if let Some(identifier) = self.review_transition_backlog_issue.as_deref() {
            labels.push(format!("backlog review transition ({identifier})"));
        }
        labels
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumeProvider {
    Claude,
    Codex,
}

impl ResumeProvider {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    pub(super) fn for_agent(agent: &str) -> Option<Self> {
        match agent {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatestResumeHandle {
    pub provider: ResumeProvider,
    pub id: String,
}

impl LatestResumeHandle {
    pub(super) fn matches_agent(&self, agent: &str) -> bool {
        ResumeProvider::for_agent(agent) == Some(self.provider)
    }
}

pub(super) fn explicit_resume_provider_label(handle: Option<&LatestResumeHandle>) -> String {
    handle
        .map(|handle| handle.provider.label().to_string())
        .unwrap_or_else(|| "unavailable".to_string())
}

pub(super) fn explicit_resume_id_label(handle: Option<&LatestResumeHandle>) -> String {
    handle
        .map(|handle| handle.id.clone())
        .unwrap_or_else(|| "unavailable".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingIssue {
    pub identifier: String,
    pub title: String,
    pub project: Option<String>,
    pub team_key: String,
}

impl From<IssueSummary> for PendingIssue {
    fn from(value: IssueSummary) -> Self {
        Self {
            identifier: value.identifier,
            title: value.title,
            project: value.project.map(|project| project.name),
            team_key: value.team.key,
        }
    }
}

/// A Linear issue currently in `In Progress`, surfaced in the dashboard In Progress Issues pane.
///
/// This is a stable, dashboard-facing view model that contains only the data the TUI needs
/// to render each row and the drill-in detail view, without requiring additional service lookups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveIssue {
    pub identifier: String,
    pub title: String,
    pub assignee: Option<String>,
    pub state_name: String,
    pub has_open_pr: bool,
    pub pr_url: Option<String>,
    pub description: Option<String>,
    pub url: String,
    pub team_key: String,
    pub project: Option<String>,
}

impl ActiveIssue {
    /// Build an `ActiveIssue` from a Linear `IssueSummary`.
    ///
    /// GitHub enrichment considers only open attached PRs. An attachment is
    /// treated as an open GitHub PR when its URL matches the `github.com/.*/pull/`
    /// pattern and the attachment metadata does not indicate a `closed` or `merged` state.
    pub fn from_issue(issue: IssueSummary) -> Self {
        let (has_open_pr, pr_url) = detect_open_github_pr(&issue.attachments);
        Self {
            identifier: issue.identifier,
            title: issue.title,
            assignee: issue.assignee.map(|a| a.name),
            state_name: issue
                .state
                .as_ref()
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "Unknown".to_string()),
            has_open_pr,
            pr_url,
            description: issue.description,
            url: issue.url,
            team_key: issue.team.key,
            project: issue.project.map(|p| p.name),
        }
    }

    pub(super) fn short_title(&self, max_len: usize) -> String {
        if self.title.len() <= max_len {
            self.title.clone()
        } else {
            format!("{}...", &self.title[..max_len.saturating_sub(3)])
        }
    }

    pub(super) fn assignee_label(&self) -> &str {
        self.assignee.as_deref().unwrap_or("unassigned")
    }

    pub(super) fn pr_label(&self) -> &'static str {
        if self.has_open_pr { "PR" } else { "-" }
    }
}

/// Returns `(has_open_pr, pr_url)` by inspecting Linear issue attachments.
///
/// An attachment is considered an open GitHub PR when its URL contains
/// `github.com/.*/pull/` and the attachment metadata does not explicitly
/// mark the state as `closed` or `merged`.
fn detect_open_github_pr(attachments: &[AttachmentSummary]) -> (bool, Option<String>) {
    for attachment in attachments {
        if !is_github_pr_url(&attachment.url) {
            continue;
        }
        if is_attachment_closed_or_merged(attachment) {
            continue;
        }
        return (true, Some(attachment.url.clone()));
    }
    (false, None)
}

fn is_github_pr_url(url: &str) -> bool {
    url.contains("github.com/") && url.contains("/pull/")
}

fn is_attachment_closed_or_merged(attachment: &AttachmentSummary) -> bool {
    if let Some(state) = attachment.metadata.get("state").and_then(|v| v.as_str()) {
        let normalized = state.to_lowercase();
        return normalized == "closed" || normalized == "merged";
    }
    if let Some(status) = attachment.metadata.get("status").and_then(|v| v.as_str()) {
        let normalized = status.to_lowercase();
        return normalized == "closed" || normalized == "merged";
    }
    false
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    #[serde(default)]
    pub input: Option<u64>,
    #[serde(default)]
    pub output: Option<u64>,
}

impl TokenUsage {
    pub(super) fn total(&self) -> Option<u64> {
        match (self.input, self.output) {
            (None, None) => None,
            (input, output) => Some(input.unwrap_or(0) + output.unwrap_or(0)),
        }
    }

    pub(super) fn accumulate(&mut self, usage: &Self) {
        if let Some(input) = usage.input {
            self.input = Some(self.input.unwrap_or(0) + input);
        }
        if let Some(output) = usage.output {
            self.output = Some(self.output.unwrap_or(0) + output);
        }
    }

    pub(super) fn display_compact(&self) -> String {
        match (self.input, self.output, self.total()) {
            (None, None, _) => "n/a".to_string(),
            (input, output, Some(total)) => format!(
                "in {} | out {} | total {}",
                input
                    .map(format_number)
                    .unwrap_or_else(|| "n/a".to_string()),
                output
                    .map(format_number)
                    .unwrap_or_else(|| "n/a".to_string()),
                format_number(total)
            ),
            (_, _, None) => "n/a".to_string(),
        }
    }

    pub(super) fn display_table_compact(&self) -> String {
        self.total()
            .map(format_number)
            .unwrap_or_else(|| "n/a".to_string())
    }

    pub(super) fn is_known(&self) -> bool {
        self.input.is_some() || self.output.is_some()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnPromptMode {
    #[default]
    FullPrompt,
    Continuation,
}

impl TurnPromptMode {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::FullPrompt => "full_prompt",
            Self::Continuation => "continuation",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnTokenSnapshot {
    pub turn: u32,
    pub prompt_mode: TurnPromptMode,
    #[serde(default)]
    pub tokens: TokenUsage,
    pub captured_at_epoch_seconds: u64,
}

impl TurnTokenSnapshot {
    pub(super) fn display_compact(&self) -> String {
        format!(
            "turn {} tokens: in {} | out {} | prompt_mode={}",
            self.turn,
            self.tokens
                .input
                .map(format_number)
                .unwrap_or_else(|| "n/a".to_string()),
            self.tokens
                .output
                .map(format_number)
                .unwrap_or_else(|| "n/a".to_string()),
            self.prompt_mode.label()
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextPressure {
    Normal,
    Elevated,
    High,
    Critical,
}

impl ContextPressure {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Elevated => "elevated",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    pub(crate) fn from_turn_history(
        turn_history: &[TurnTokenSnapshot],
        context_budget_tokens: u64,
    ) -> Self {
        let known_input_tokens = completed_turn_known_input_tokens(turn_history);
        if known_input_tokens == 0 || context_budget_tokens == 0 {
            return Self::Normal;
        }

        let usage_percent = known_input_tokens.saturating_mul(100);
        let critical_threshold = context_budget_tokens.saturating_mul(95);
        let high_threshold = context_budget_tokens.saturating_mul(85);
        let elevated_threshold = context_budget_tokens.saturating_mul(70);

        if usage_percent >= critical_threshold {
            Self::Critical
        } else if usage_percent >= high_threshold {
            Self::High
        } else if usage_percent >= elevated_threshold {
            Self::Elevated
        } else {
            Self::Normal
        }
    }
}

pub(crate) fn completed_turn_known_input_tokens(turn_history: &[TurnTokenSnapshot]) -> u64 {
    turn_history
        .iter()
        .map(|snapshot| snapshot.tokens.input.unwrap_or(0))
        .sum()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalSessionData {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub tokens: TokenUsage,
    #[serde(default)]
    pub repair: Option<CanonicalRepairRecord>,
}

impl CanonicalSessionData {
    pub(super) fn provider_label(&self) -> Option<&str> {
        self.provider.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionTimeoutTermination {
    Sigterm,
    Sigkill,
}

impl SessionTimeoutTermination {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Sigterm => "sigterm",
            Self::Sigkill => "sigkill",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTimeoutRecord {
    pub turn: u32,
    pub pid: u32,
    pub elapsed_seconds: u64,
    pub timeout_seconds: u64,
    pub graceful_shutdown_seconds: u64,
    pub termination: SessionTimeoutTermination,
}

impl SessionTimeoutRecord {
    pub(super) fn summary_label(&self) -> String {
        format!(
            "turn {} timeout | elapsed {}s | limit {}s | pid {} | {}",
            self.turn,
            self.elapsed_seconds,
            self.timeout_seconds,
            self.pid,
            self.termination.label()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalRepairRecord {
    pub status: CanonicalRepairStatus,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalRepairStatus {
    Recovered,
    Skipped,
}

/// Distinguishes whether a session was created by the continuous `listen` daemon
/// or by a one-off `agents execute` invocation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionOrigin {
    /// Session was created by `meta agents listen` (default for backwards compatibility).
    #[default]
    Listen,
    /// Session was created by `meta agents execute <ISSUE_ID>`.
    Execute,
}

impl SessionOrigin {
    pub fn display_label(self) -> &'static str {
        match self {
            Self::Listen => "Listen",
            Self::Execute => "Execute",
        }
    }

    pub fn is_execute(self) -> bool {
        matches!(self, Self::Execute)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestStatus {
    #[default]
    Unpublished,
    Draft,
    Ready,
}

impl PullRequestStatus {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Unpublished => "none",
            Self::Draft => "draft",
            Self::Ready => "ready",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestSummary {
    #[serde(default)]
    pub number: Option<u64>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub status: PullRequestStatus,
}

impl PullRequestSummary {
    pub(super) fn compact_label(&self) -> String {
        match (self.status, self.number) {
            (PullRequestStatus::Unpublished, _) => "none".to_string(),
            (PullRequestStatus::Draft, Some(number)) => format!("draft #{number}"),
            (PullRequestStatus::Ready, Some(number)) => format!("ready #{number}"),
            (PullRequestStatus::Draft, None) => "draft".to_string(),
            (PullRequestStatus::Ready, None) => "ready".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleWorkerFailure {
    pub pid: u32,
    pub observed_at_epoch_seconds: u64,
    pub last_persisted_phase: SessionPhase,
    pub summary: String,
    pub classification: BlockedReason,
}

impl StaleWorkerFailure {
    pub(super) fn operator_summary(&self) -> String {
        format!(
            "pid {} | {} | {} | retryable {}",
            self.pid,
            self.last_persisted_phase.display_label(),
            self.summary,
            if self.classification.retryable {
                "yes"
            } else {
                "no"
            }
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    #[serde(default)]
    pub issue_id: Option<String>,
    pub issue_identifier: String,
    pub issue_title: String,
    pub project_name: Option<String>,
    pub team_key: String,
    pub issue_url: String,
    pub phase: SessionPhase,
    pub summary: String,
    #[serde(default)]
    pub blocked: Option<BlockedReason>,
    pub brief_path: Option<String>,
    #[serde(default)]
    pub backlog_issue_identifier: Option<String>,
    #[serde(default)]
    pub backlog_issue_title: Option<String>,
    #[serde(default)]
    pub backlog_path: Option<String>,
    #[serde(default)]
    pub workspace_path: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub pull_request: PullRequestSummary,
    #[serde(default)]
    pub workpad_comment_id: Option<String>,
    #[serde(default)]
    pub started_at_epoch_seconds: u64,
    pub updated_at_epoch_seconds: u64,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub latest_resume_handle: Option<LatestResumeHandle>,
    #[serde(default)]
    pub context_budget_tokens: Option<u64>,
    #[serde(default)]
    pub pending_linear_sync: Option<PendingLinearSync>,
    #[serde(default)]
    pub stale_worker_recovery_attempt_count: u32,
    #[serde(default)]
    pub latest_stale_worker_failure: Option<StaleWorkerFailure>,
    #[serde(default)]
    pub last_timeout: Option<SessionTimeoutRecord>,
    #[serde(default)]
    pub turns: Option<u32>,
    #[serde(default)]
    pub tokens: TokenUsage,
    #[serde(default)]
    pub turn_history: Vec<TurnTokenSnapshot>,
    #[serde(default)]
    pub canonical: CanonicalSessionData,
    #[serde(default)]
    pub log_path: Option<String>,
    #[serde(default)]
    pub origin: SessionOrigin,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Categorizes the primary reason a listen session is blocked.
pub enum BlockedCategory {
    /// Missing setup prerequisites blocked the session before useful execution could continue.
    Setup,
    /// A worker turn failed or exhausted its available execution budget.
    Turn,
    /// Review, verification, or validation gates blocked the session.
    Gate,
    /// Infrastructure, dependency, or orchestration recovery blocked the session.
    Infra,
    /// The session is blocked for a reason that does not fit one of the explicit categories.
    #[default]
    Other,
}

impl BlockedCategory {
    fn display_label(self) -> &'static str {
        match self {
            Self::Setup => "Setup",
            Self::Turn => "Turn",
            Self::Gate => "Gate",
            Self::Infra => "Infra",
            Self::Other => "Blocked",
        }
    }

    fn stage_label(self) -> &'static str {
        match self {
            Self::Setup => "Setup Err",
            Self::Turn => "Turn Err",
            Self::Gate => "Gate Err",
            Self::Infra => "Infra Err",
            Self::Other => "Blocked",
        }
    }

    fn suggested_action(self, retryable: bool) -> &'static str {
        match (self, retryable) {
            (Self::Setup, true) => {
                "Restore the missing setup prerequisites, then retry the session."
            }
            (Self::Setup, false) => "Fix the workspace or tool setup before retrying this session.",
            (Self::Turn, true) => "Inspect the worker log, repair the turn failure, then retry.",
            (Self::Turn, false) => {
                "Investigate the turn failure and adjust the ticket plan before retrying."
            }
            (Self::Gate, true) => {
                "Resolve the review or validation gate failure, then retry the session."
            }
            (Self::Gate, false) => {
                "Repair the blocking review or validation failure before retrying."
            }
            (Self::Infra, true) => {
                "Wait for the dependency or supervisor to recover, then retry the session."
            }
            (Self::Infra, false) => "Restore the dependency or orchestration path before retrying.",
            (Self::Other, true) => "Inspect the session log and retry once the blocker is cleared.",
            (Self::Other, false) => {
                "Inspect the session log and resolve the blocker before retrying."
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Persists the structured blocked taxonomy alongside a blocked listen session.
pub struct BlockedReason {
    /// The high-level blocked category used for labels and summary counts.
    pub category: BlockedCategory,
    /// The human-readable explanation for the current blocked condition.
    pub reason: String,
    /// Whether the blocked condition is safe to retry without prerequisite changes.
    pub retryable: bool,
}

impl BlockedReason {
    pub(super) fn new(
        category: BlockedCategory,
        reason: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            category,
            reason: reason.into(),
            retryable,
        }
    }

    pub(super) fn summary_headline(&self) -> String {
        let reason = self.reason.trim();
        if reason.is_empty() {
            "Blocked".to_string()
        } else {
            format!("Blocked | {reason}")
        }
    }

    pub(super) fn stage_label(&self) -> &'static str {
        self.category.stage_label()
    }

    pub(super) fn category_label(&self) -> &'static str {
        self.category.display_label()
    }

    pub(super) fn suggested_action(&self) -> &'static str {
        self.category.suggested_action(self.retryable)
    }
}

impl AgentSession {
    pub(super) fn issue_matches(&self, identifier: &str) -> bool {
        self.issue_identifier.eq_ignore_ascii_case(identifier)
    }

    pub(super) fn stage_label(&self) -> String {
        self.blocked
            .as_ref()
            .map(|blocked| blocked.stage_label().to_string())
            .unwrap_or_else(|| self.phase.display_label().to_string())
    }

    pub(super) fn blocked_category_label(&self) -> Option<&'static str> {
        self.blocked.as_ref().map(BlockedReason::category_label)
    }

    pub(super) fn blocked_retry_label(&self) -> Option<&'static str> {
        self.blocked
            .as_ref()
            .map(|blocked| if blocked.retryable { "yes" } else { "no" })
    }

    pub(super) fn blocked_suggested_action(&self) -> Option<&'static str> {
        self.blocked.as_ref().map(BlockedReason::suggested_action)
    }

    pub(super) fn pid_label(&self) -> String {
        self.pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "-".to_string())
    }

    pub(super) fn age_label(&self, now_epoch_seconds: u64) -> String {
        format_duration(now_epoch_seconds.saturating_sub(self.updated_at_epoch_seconds))
    }

    pub(super) fn elapsed_since_start_label(&self, now_epoch_seconds: u64) -> String {
        format_duration(now_epoch_seconds.saturating_sub(self.started_at_epoch_seconds))
    }

    pub(super) fn table_tokens_label(&self) -> String {
        self.canonical_tokens().display_table_compact()
    }

    pub(super) fn session_label(&self) -> String {
        self.latest_resume_handle
            .as_ref()
            .map(|resume| compact_identifier(&resume.id))
            .unwrap_or_else(|| "-".to_string())
    }

    pub(super) fn origin_label(&self) -> &'static str {
        self.origin.display_label()
    }

    pub(super) fn latest_resume_provider_label(&self) -> String {
        self.latest_resume_handle
            .as_ref()
            .map(|resume| resume.provider.label().to_string())
            .unwrap_or_else(|| "-".to_string())
    }

    pub(super) fn pull_request_label(&self) -> String {
        self.pull_request.compact_label()
    }

    pub(super) fn pending_linear_sync_label(&self) -> Option<String> {
        self.pending_linear_sync.as_ref().map(|pending| {
            let operations = pending.operation_labels().join(", ");
            match pending.last_failure.as_ref() {
                Some(failure) => format!("{} | {}", operations, failure.kind.label()),
                None => operations,
            }
        })
    }

    pub(super) fn last_timeout_label(&self) -> Option<String> {
        self.last_timeout
            .as_ref()
            .map(SessionTimeoutRecord::summary_label)
    }

    pub(super) fn canonical_tokens(&self) -> &TokenUsage {
        if self.canonical.tokens.is_known() {
            &self.canonical.tokens
        } else {
            &self.tokens
        }
    }

    pub(super) fn provider_label(&self) -> String {
        self.canonical
            .provider_label()
            .map(str::to_string)
            .or_else(|| {
                self.latest_resume_handle
                    .as_ref()
                    .map(|resume| resume.provider.label().to_string())
            })
            .unwrap_or_else(|| "unavailable".to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    Claimed,
    BriefReady,
    Running,
    Reviewing,
    FinalReview,
    Verifying,
    Validating,
    Publishing,
    Paused,
    Completed,
    Blocked,
}

impl SessionPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Claimed => "claimed",
            Self::BriefReady => "brief-ready",
            Self::Running => "running",
            Self::Reviewing => "reviewing",
            Self::FinalReview => "final-review",
            Self::Verifying => "verifying",
            Self::Validating => "validating",
            Self::Publishing => "publishing",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Blocked => "blocked",
        }
    }

    pub fn display_label(self) -> &'static str {
        match self {
            Self::Claimed => "Claimed",
            Self::BriefReady => "Brief Ready",
            Self::Running => "Running",
            Self::Reviewing => "Reviewing",
            Self::FinalReview => "Final Review",
            Self::Verifying => "Verifying",
            Self::Validating => "Validating",
            Self::Publishing => "Publishing",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::Blocked => "Blocked",
        }
    }

    #[cfg(test)]
    pub fn html_class(self) -> &'static str {
        match self {
            Self::Claimed => "warning",
            Self::BriefReady => "active",
            Self::Running => "active",
            Self::Reviewing => "active",
            Self::FinalReview => "active",
            Self::Verifying => "active",
            Self::Validating => "active",
            Self::Publishing => "active",
            Self::Paused => "warning",
            Self::Completed => "success",
            Self::Blocked => "danger",
        }
    }

    pub fn is_completed(self) -> bool {
        matches!(self, Self::Completed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ListenState {
    version: u8,
    #[serde(default)]
    pub(super) pending_issues: Vec<PendingIssue>,
    #[serde(default)]
    pub(super) active_issues: Vec<ActiveIssue>,
    #[serde(default)]
    pub(super) degraded: Option<LinearFailureSnapshot>,
    pub(super) sessions: Vec<AgentSession>,
}

impl Default for ListenState {
    fn default() -> Self {
        Self {
            version: 1,
            pending_issues: Vec::new(),
            active_issues: Vec::new(),
            degraded: None,
            sessions: Vec::new(),
        }
    }
}

impl ListenState {
    #[cfg(test)]
    pub(super) fn from_sessions(sessions: Vec<AgentSession>) -> Self {
        Self {
            version: 1,
            pending_issues: Vec::new(),
            active_issues: Vec::new(),
            degraded: None,
            sessions,
        }
    }

    pub(super) fn blocks_pickup(&self, identifier: &str) -> bool {
        self.sessions.iter().any(|session| {
            session.issue_matches(identifier)
                && matches!(
                    session.phase,
                    SessionPhase::Claimed
                        | SessionPhase::BriefReady
                        | SessionPhase::Running
                        | SessionPhase::Verifying
                        | SessionPhase::Validating
                        | SessionPhase::Paused
                        | SessionPhase::Completed
                        | SessionPhase::Blocked
                )
        })
    }

    pub(super) fn upsert(&mut self, session: AgentSession) {
        if let Some(existing) = self
            .sessions
            .iter_mut()
            .find(|existing| existing.issue_matches(&session.issue_identifier))
        {
            *existing = session;
        } else {
            self.sessions.push(session);
        }
    }

    pub(super) fn remove_sessions<F>(&mut self, mut predicate: F) -> Vec<AgentSession>
    where
        F: FnMut(&AgentSession) -> bool,
    {
        let mut removed = Vec::new();
        self.sessions.retain(|session| {
            if predicate(session) {
                removed.push(session.clone());
                false
            } else {
                true
            }
        });
        removed
    }

    pub(super) fn prune_completed_sessions_older_than(
        &mut self,
        now_epoch_seconds: u64,
        ttl_seconds: u64,
    ) -> Vec<AgentSession> {
        self.remove_sessions(|session| {
            session.phase.is_completed()
                && now_epoch_seconds.saturating_sub(session.updated_at_epoch_seconds) > ttl_seconds
        })
    }

    pub(super) fn remove_issue(&mut self, identifier: &str) -> bool {
        let original_len = self.sessions.len();
        self.sessions
            .retain(|session| !session.issue_matches(identifier));
        self.sessions.len() != original_len
    }

    pub(super) fn sorted_sessions(&self) -> Vec<AgentSession> {
        let mut sessions = self.sessions.clone();
        sessions.sort_by(|left, right| {
            right
                .updated_at_epoch_seconds
                .cmp(&left.updated_at_epoch_seconds)
                .then_with(|| left.issue_identifier.cmp(&right.issue_identifier))
        });
        sessions
    }

    pub(super) fn latest_session(&self) -> Option<AgentSession> {
        self.sorted_sessions().into_iter().next()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AgentSession, CanonicalSessionData, ContextPressure, LatestResumeHandle, PullRequestStatus,
        PullRequestSummary, ResumeProvider, SessionOrigin, SessionPhase, TokenUsage,
        TurnPromptMode, TurnTokenSnapshot, completed_turn_known_input_tokens,
        explicit_resume_id_label, explicit_resume_provider_label,
    };

    fn session() -> AgentSession {
        AgentSession {
            issue_id: Some("issue-1".to_string()),
            issue_identifier: "ENG-10194".to_string(),
            issue_title: "Capture listen resume IDs".to_string(),
            project_name: Some("MetaStack CLI".to_string()),
            team_key: "ENG".to_string(),
            issue_url: "https://linear.app/issues/ENG-10194".to_string(),
            phase: SessionPhase::Running,
            summary: "Running".to_string(),
            blocked: None,
            brief_path: None,
            backlog_issue_identifier: None,
            backlog_issue_title: None,
            backlog_path: None,
            workspace_path: None,
            branch: None,
            pull_request: PullRequestSummary::default(),
            workpad_comment_id: None,
            started_at_epoch_seconds: 1,
            updated_at_epoch_seconds: 1,
            pid: None,
            session_id: Some("issue-1".to_string()),
            latest_resume_handle: None,
            context_budget_tokens: None,
            pending_linear_sync: None,
            stale_worker_recovery_attempt_count: 0,
            latest_stale_worker_failure: None,
            last_timeout: None,
            turns: Some(1),
            tokens: TokenUsage::default(),
            turn_history: Vec::new(),
            canonical: CanonicalSessionData::default(),
            log_path: None,
            origin: SessionOrigin::Listen,
        }
    }

    fn turn_snapshot(turn: u32, input_tokens: Option<u64>) -> TurnTokenSnapshot {
        TurnTokenSnapshot {
            turn,
            prompt_mode: TurnPromptMode::FullPrompt,
            tokens: TokenUsage {
                input: input_tokens,
                output: None,
            },
            captured_at_epoch_seconds: 1,
        }
    }

    #[test]
    fn session_origin_defaults_to_listen() {
        let session = session();
        assert_eq!(session.origin, SessionOrigin::Listen);
        assert_eq!(session.origin_label(), "Listen");
        assert!(!session.origin.is_execute());
    }

    #[test]
    fn session_origin_execute_label() {
        let mut session = session();
        session.origin = SessionOrigin::Execute;
        assert_eq!(session.origin_label(), "Execute");
        assert!(session.origin.is_execute());
    }

    #[test]
    fn session_label_uses_latest_resume_handle_only() {
        let mut session = session();
        assert_eq!(session.session_label(), "-");

        session.latest_resume_handle = Some(LatestResumeHandle {
            provider: ResumeProvider::Codex,
            id: "019cedb4-2293-7651-b0b4-dfac4af6a640".to_string(),
        });

        assert_eq!(session.session_label(), "019c...f6a640");
        assert_eq!(session.latest_resume_provider_label(), "codex");
        assert_eq!(
            session
                .latest_resume_handle
                .as_ref()
                .map(|resume| resume.id.as_str()),
            Some("019cedb4-2293-7651-b0b4-dfac4af6a640")
        );
    }

    #[test]
    fn explicit_resume_labels_share_unavailable_and_full_id_formatting() {
        assert_eq!(explicit_resume_provider_label(None), "unavailable");
        assert_eq!(explicit_resume_id_label(None), "unavailable");

        let handle = LatestResumeHandle {
            provider: ResumeProvider::Claude,
            id: "provider-resume-123".to_string(),
        };
        assert_eq!(
            explicit_resume_provider_label(Some(&handle)),
            "claude".to_string()
        );
        assert_eq!(
            explicit_resume_id_label(Some(&handle)),
            "provider-resume-123".to_string()
        );
    }

    #[test]
    fn session_pull_request_label_stays_compact() {
        let mut session = session();
        assert_eq!(session.pull_request_label(), "none");

        session.pull_request = PullRequestSummary {
            number: Some(321),
            url: Some("https://github.com/metastack-labs/metastack-cli/pull/321".to_string()),
            status: PullRequestStatus::Draft,
        };
        assert_eq!(session.pull_request_label(), "draft #321");

        session.pull_request.status = PullRequestStatus::Ready;
        assert_eq!(session.pull_request_label(), "ready #321");
    }

    #[test]
    fn pull_request_summary_compact_label_surfaces_ready_status() {
        let pull_request = PullRequestSummary {
            number: Some(321),
            url: Some("https://github.com/metastack-labs/metastack-cli/pull/321".to_string()),
            status: PullRequestStatus::Ready,
        };

        assert_eq!(pull_request.compact_label(), "ready #321");
    }

    #[test]
    fn session_table_tokens_label_prefers_total_only() {
        let mut session = session();
        assert_eq!(session.table_tokens_label(), "n/a");

        session.tokens = TokenUsage {
            input: Some(12_300),
            output: Some(40),
        };
        assert_eq!(session.table_tokens_label(), "12,340");
    }

    #[test]
    fn active_issue_detects_open_github_pr_from_attachments() {
        use crate::linear::{AttachmentSummary, IssueSummary, TeamRef, WorkflowState};
        use serde_json::json;

        let issue = IssueSummary {
            id: "issue-1".to_string(),
            identifier: "MET-99".to_string(),
            title: "Active ticket with open PR".to_string(),
            description: Some("A description".to_string()),
            url: "https://linear.app/issues/MET-99".to_string(),
            priority: None,
            estimate: None,
            updated_at: "2026-03-21T00:00:00Z".to_string(),
            team: TeamRef {
                key: "MET".to_string(),
                id: "team-1".to_string(),
                name: "MetaStack".to_string(),
            },
            project: None,
            assignee: Some(crate::linear::UserRef {
                id: "user-1".to_string(),
                name: "Alice".to_string(),
                email: None,
            }),
            labels: vec![],
            comments: vec![],
            state: Some(WorkflowState {
                id: "state-1".to_string(),
                name: "In Progress".to_string(),
                kind: Some("started".to_string()),
            }),
            attachments: vec![AttachmentSummary {
                id: "att-1".to_string(),
                title: "PR #42".to_string(),
                url: "https://github.com/org/repo/pull/42".to_string(),
                source_type: Some("github".to_string()),
                metadata: json!({}),
            }],
            parent: None,
            children: vec![],
        };

        let active = super::ActiveIssue::from_issue(issue);
        assert!(active.has_open_pr);
        assert_eq!(
            active.pr_url.as_deref(),
            Some("https://github.com/org/repo/pull/42")
        );
        assert_eq!(active.assignee.as_deref(), Some("Alice"));
        assert_eq!(active.state_name, "In Progress");
    }

    #[test]
    fn active_issue_ignores_closed_pr_attachments() {
        use crate::linear::{AttachmentSummary, IssueSummary, TeamRef, WorkflowState};
        use serde_json::json;

        let issue = IssueSummary {
            id: "issue-2".to_string(),
            identifier: "MET-100".to_string(),
            title: "Issue with closed PR".to_string(),
            description: None,
            url: "https://linear.app/issues/MET-100".to_string(),
            priority: None,
            estimate: None,
            updated_at: "2026-03-21T00:00:00Z".to_string(),
            team: TeamRef {
                key: "MET".to_string(),
                id: "team-1".to_string(),
                name: "MetaStack".to_string(),
            },
            project: None,
            assignee: None,
            labels: vec![],
            comments: vec![],
            state: Some(WorkflowState {
                id: "state-1".to_string(),
                name: "In Progress".to_string(),
                kind: Some("started".to_string()),
            }),
            attachments: vec![AttachmentSummary {
                id: "att-2".to_string(),
                title: "PR #43".to_string(),
                url: "https://github.com/org/repo/pull/43".to_string(),
                source_type: Some("github".to_string()),
                metadata: json!({"state": "closed"}),
            }],
            parent: None,
            children: vec![],
        };

        let active = super::ActiveIssue::from_issue(issue);
        assert!(!active.has_open_pr);
        assert!(active.pr_url.is_none());
        assert_eq!(active.assignee_label(), "unassigned");
    }

    #[test]
    fn active_issue_ignores_merged_pr_attachments() {
        use crate::linear::{AttachmentSummary, IssueSummary, TeamRef, WorkflowState};
        use serde_json::json;

        let issue = IssueSummary {
            id: "issue-3".to_string(),
            identifier: "MET-101".to_string(),
            title: "Issue with merged PR".to_string(),
            description: None,
            url: "https://linear.app/issues/MET-101".to_string(),
            priority: None,
            estimate: None,
            updated_at: "2026-03-21T00:00:00Z".to_string(),
            team: TeamRef {
                key: "MET".to_string(),
                id: "team-1".to_string(),
                name: "MetaStack".to_string(),
            },
            project: None,
            assignee: None,
            labels: vec![],
            comments: vec![],
            state: Some(WorkflowState {
                id: "state-1".to_string(),
                name: "In Progress".to_string(),
                kind: Some("started".to_string()),
            }),
            attachments: vec![AttachmentSummary {
                id: "att-3".to_string(),
                title: "PR #44".to_string(),
                url: "https://github.com/org/repo/pull/44".to_string(),
                source_type: Some("github".to_string()),
                metadata: json!({"state": "merged"}),
            }],
            parent: None,
            children: vec![],
        };

        let active = super::ActiveIssue::from_issue(issue);
        assert!(!active.has_open_pr);
        assert!(active.pr_url.is_none());
    }

    #[test]
    fn active_issue_short_title_truncates_long_titles() {
        let active = super::ActiveIssue {
            identifier: "MET-1".to_string(),
            title: "A very long title that should be truncated for the table view".to_string(),
            assignee: None,
            state_name: "In Progress".to_string(),
            has_open_pr: false,
            pr_url: None,
            description: None,
            url: "https://linear.app/issues/MET-1".to_string(),
            team_key: "MET".to_string(),
            project: None,
        };
        let short = active.short_title(20);
        assert!(short.len() <= 20);
        assert!(short.ends_with("..."));
    }

    #[test]
    fn active_issue_pr_label_shows_presence() {
        let mut active = super::ActiveIssue {
            identifier: "MET-1".to_string(),
            title: "Test".to_string(),
            assignee: None,
            state_name: "In Progress".to_string(),
            has_open_pr: true,
            pr_url: Some("https://github.com/org/repo/pull/1".to_string()),
            description: None,
            url: "https://linear.app/issues/MET-1".to_string(),
            team_key: "MET".to_string(),
            project: None,
        };
        assert_eq!(active.pr_label(), "PR");

        active.has_open_pr = false;
        assert_eq!(active.pr_label(), "-");
    }

    #[test]
    fn context_pressure_thresholds_are_exact() {
        let budget = 100;

        assert_eq!(
            ContextPressure::from_turn_history(&[turn_snapshot(1, Some(69))], budget),
            ContextPressure::Normal
        );
        assert_eq!(
            ContextPressure::from_turn_history(&[turn_snapshot(1, Some(70))], budget),
            ContextPressure::Elevated
        );
        assert_eq!(
            ContextPressure::from_turn_history(&[turn_snapshot(1, Some(84))], budget),
            ContextPressure::Elevated
        );
        assert_eq!(
            ContextPressure::from_turn_history(&[turn_snapshot(1, Some(85))], budget),
            ContextPressure::High
        );
        assert_eq!(
            ContextPressure::from_turn_history(&[turn_snapshot(1, Some(94))], budget),
            ContextPressure::High
        );
        assert_eq!(
            ContextPressure::from_turn_history(&[turn_snapshot(1, Some(95))], budget),
            ContextPressure::Critical
        );
    }

    #[test]
    fn context_pressure_counts_only_known_completed_turn_input_tokens() {
        let turn_history = vec![
            turn_snapshot(1, Some(40)),
            turn_snapshot(2, None),
            turn_snapshot(3, Some(45)),
        ];

        assert_eq!(completed_turn_known_input_tokens(&turn_history), 85);
        assert_eq!(
            ContextPressure::from_turn_history(&turn_history, 100),
            ContextPressure::High
        );
    }

    #[test]
    fn context_pressure_stays_normal_when_all_input_telemetry_is_missing() {
        let turn_history = vec![turn_snapshot(1, None), turn_snapshot(2, None)];

        assert_eq!(completed_turn_known_input_tokens(&turn_history), 0);
        assert_eq!(
            ContextPressure::from_turn_history(&turn_history, 100),
            ContextPressure::Normal
        );
    }
}

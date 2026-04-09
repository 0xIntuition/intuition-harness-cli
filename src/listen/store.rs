use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent_provider::builtin_provider_adapter;
use crate::config::{
    AppConfig, DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS, PlanningMeta, resolve_data_root,
};
use crate::fs::{PlanningPaths, canonicalize_existing_dir, ensure_dir};
use crate::listen::compact_session_summary;
use crate::session_runtime::{
    ActiveSessionFile, WorkflowRootLayout, read_json, read_optional_json_lossy, write_json,
};

use super::state::{
    AgentSession, BlockedReason, COMPLETED_SESSION_TTL_SECONDS, CanonicalRepairRecord,
    CanonicalRepairStatus, CanonicalSessionData, LatestResumeHandle, ListenState,
    PendingLinearSync, PullRequestStatus, PullRequestSummary, SessionPhase, StaleWorkerFailure,
    TokenUsage, TurnTokenSnapshot,
};
use super::verification::{VerificationReport, VerificationSummary};

const LISTEN_STORE_VERSION: u8 = 1;
const LISTEN_SESSION_DETAIL_VERSION: u8 = 5;
const LOG_EXCERPT_LIMIT: usize = 6;
const LOG_EXCERPT_MAX_CHARS: usize = 120;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

fn default_context_budget_tokens() -> u64 {
    DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS
}
#[cfg(test)]
fn listen_turn_log_prefix() -> String {
    format!("--- {} listen turn ", crate::branding::COMMAND_NAME)
}

fn is_supported_listen_log_header(line: &str, suffix: &str) -> bool {
    line.strip_prefix("--- ")
        .and_then(|value| {
            value
                .strip_prefix(crate::branding::COMMAND_NAME)
                .or_else(|| value.strip_prefix("meta"))
        })
        .is_some_and(|value| value.starts_with(suffix))
}

fn is_listen_turn_log_header(line: &str) -> bool {
    is_supported_listen_log_header(line, " listen turn ")
}

fn is_listen_preflight_failure_log_header(line: &str) -> bool {
    is_supported_listen_log_header(line, " listen preflight failed @ ")
}

#[derive(Debug, Clone)]
pub(crate) struct ListenProjectStore {
    identity: ListenProjectIdentity,
    paths: ListenProjectPaths,
}

#[derive(Debug, Clone)]
pub(super) struct ListenProjectIdentity {
    pub(super) project_key: String,
    pub(super) source_root: PathBuf,
    pub(super) metastack_root: PathBuf,
    pub(super) source_label: String,
    pub(super) project_selector: Option<String>,
    pub(super) project_label: String,
}

#[derive(Debug, Clone)]
pub(super) struct ListenProjectPaths {
    pub(super) layout: WorkflowRootLayout,
    pub(super) projects_root: PathBuf,
    pub(super) project_dir: PathBuf,
    pub(super) project_metadata_path: PathBuf,
    pub(super) state_path: PathBuf,
    pub(super) lock_path: PathBuf,
    pub(super) logs_dir: PathBuf,
    pub(super) details_dir: PathBuf,
    pub(super) verification_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ListenProjectMetadata {
    pub(super) version: u8,
    pub(super) project_key: String,
    #[serde(default)]
    pub(super) project_selector: Option<String>,
    pub(super) project_label: String,
    pub(super) source_root: String,
    pub(super) metastack_root: String,
    #[serde(default)]
    pub(super) source_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ActiveListenerLock {
    pub(super) pid: u32,
    pub(super) acquired_at_epoch_seconds: u64,
    pub(super) source_root: String,
    pub(super) metastack_root: String,
}

#[derive(Debug, Clone)]
pub(super) struct StoredListenProjectSummary {
    pub(super) metadata: ListenProjectMetadata,
    pub(super) state_path: PathBuf,
    pub(super) lock_path: PathBuf,
    pub(super) logs_dir: PathBuf,
    pub(super) latest_session: Option<AgentSession>,
    pub(super) active_lock: Option<ActiveListenerLock>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SessionDetailReferences {
    #[serde(default)]
    pub workspace_path: Option<String>,
    #[serde(default)]
    pub backlog_path: Option<String>,
    #[serde(default)]
    pub brief_path: Option<String>,
    #[serde(default)]
    pub workpad_comment_id: Option<String>,
    #[serde(default)]
    pub log_path: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub verification_json_path: Option<String>,
    #[serde(default)]
    pub verification_markdown_path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SessionContextReference {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SessionMilestone {
    pub at_epoch_seconds: u64,
    pub phase: SessionPhase,
    pub summary: String,
    #[serde(default)]
    pub turns: Option<u32>,
    #[serde(default)]
    pub pull_request_status: PullRequestStatus,
    #[serde(default)]
    pub pull_request_number: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SessionLogExcerpt {
    pub line_number: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ListenSessionDetail {
    pub(super) version: u8,
    pub(super) issue_identifier: String,
    pub(super) issue_title: String,
    #[serde(default)]
    pub started_at_epoch_seconds: u64,
    pub(super) updated_at_epoch_seconds: u64,
    pub(super) session_updated_at_epoch_seconds: u64,
    pub(super) phase: SessionPhase,
    pub(super) summary: String,
    #[serde(default)]
    pub blocked: Option<BlockedReason>,
    #[serde(default)]
    pub turns: Option<u32>,
    #[serde(default)]
    pub tokens: TokenUsage,
    #[serde(default)]
    pub turn_history: Vec<TurnTokenSnapshot>,
    #[serde(default)]
    pub context_budget_tokens: Option<u64>,
    #[serde(default)]
    pub canonical: CanonicalSessionData,
    #[serde(default)]
    pub pull_request: PullRequestSummary,
    #[serde(default)]
    pub latest_resume_handle: Option<LatestResumeHandle>,
    #[serde(default)]
    pub verification: Option<VerificationSummary>,
    #[serde(default)]
    pub pending_linear_sync: Option<PendingLinearSync>,
    #[serde(default)]
    pub stale_worker_recovery_attempt_count: u32,
    #[serde(default)]
    pub latest_stale_worker_failure: Option<StaleWorkerFailure>,
    #[serde(default)]
    pub last_timeout: Option<super::SessionTimeoutRecord>,
    #[serde(default)]
    pub references: SessionDetailReferences,
    #[serde(default)]
    pub prompt_context: Vec<SessionContextReference>,
    #[serde(default)]
    pub milestones: Vec<SessionMilestone>,
    #[serde(default)]
    pub log_excerpts: Vec<SessionLogExcerpt>,
}

impl ListenSessionDetail {
    pub(crate) fn context_budget_tokens(&self) -> u64 {
        self.context_budget_tokens
            .unwrap_or_else(default_context_budget_tokens)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SessionSelector {
    IssueIdentifier(String),
    Blocked,
    Completed,
    Stale,
    All,
}

impl SessionSelector {
    pub(super) fn display_label(&self) -> String {
        match self {
            Self::IssueIdentifier(identifier) => format!("issue `{identifier}`"),
            Self::Blocked => "`--blocked`".to_string(),
            Self::Completed => "`--completed`".to_string(),
            Self::Stale => "`--stale`".to_string(),
            Self::All => "`--all`".to_string(),
        }
    }

    fn matches(&self, session: &AgentSession) -> bool {
        match self {
            Self::IssueIdentifier(identifier) => session.issue_matches(identifier),
            Self::Blocked => matches!(session.phase, SessionPhase::Blocked),
            Self::Completed => matches!(session.phase, SessionPhase::Completed),
            Self::Stale => session.pid.is_some_and(|pid| !pid_is_running(pid)),
            Self::All => true,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct SessionClearOutcome {
    pub(super) cleared_sessions: Vec<AgentSession>,
    pub(super) remaining_sessions: usize,
}

#[derive(Debug)]
pub(super) struct ListenerLockGuard {
    lock_path: PathBuf,
    project_label: String,
    identity: Option<FileIdentity>,
}

#[derive(Debug)]
enum ActiveLockInspection {
    Missing,
    Present(LockSnapshot),
    Invalid {
        identity: Option<FileIdentity>,
        error: anyhow::Error,
    },
}

#[derive(Debug)]
struct LockSnapshot {
    lock: ActiveListenerLock,
    identity: Option<FileIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConditionalRemoveOutcome {
    Removed,
    Missing,
    Replaced,
}

impl ListenProjectStore {
    /// Resolves the install-scoped listen project store for the provided repository root.
    ///
    /// Returns an error when the repository root or install-scoped data root cannot be resolved.
    pub(crate) fn resolve(root: &Path, project_selector: Option<&str>) -> Result<Self> {
        let data_root = resolve_data_root()?;
        Self::resolve_with_data_root(root, data_root, project_selector)
    }

    fn resolve_with_data_root(
        root: &Path,
        data_root: PathBuf,
        project_selector: Option<&str>,
    ) -> Result<Self> {
        let identity = resolve_project_identity(root, project_selector)?;
        let projects_root = data_root.join("listen").join("projects");
        let project_dir = projects_root.join(&identity.project_key);
        let layout =
            WorkflowRootLayout::install_scoped(project_dir.clone(), "active-listener.lock.json");
        let paths = ListenProjectPaths {
            layout: layout.clone(),
            projects_root,
            project_dir: project_dir.clone(),
            project_metadata_path: layout.path("project.json"),
            state_path: layout.path("session.json"),
            lock_path: layout.active_session_path().to_path_buf(),
            logs_dir: layout.path("logs"),
            details_dir: layout.path("session-details"),
            verification_dir: layout.path("verification"),
        };

        Ok(Self { identity, paths })
    }

    pub(super) fn from_project_key(project_key: &str) -> Result<Self> {
        let data_root = resolve_data_root()?;
        let project_dir = data_root.join("listen").join("projects").join(project_key);
        let metadata_path = project_dir.join("project.json");
        let metadata = read_required_json_with_recovery::<ListenProjectMetadata>(
            &metadata_path,
            project_key,
            "listen project metadata",
        )?;
        let source_label = if metadata.source_label.trim().is_empty() {
            Path::new(&metadata.source_root)
                .file_name()
                .and_then(OsStr::to_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("project")
                .to_string()
        } else {
            metadata.source_label.clone()
        };
        let identity = ListenProjectIdentity {
            project_key: metadata.project_key.clone(),
            source_root: PathBuf::from(&metadata.source_root),
            metastack_root: PathBuf::from(&metadata.metastack_root),
            source_label,
            project_selector: metadata.project_selector.clone(),
            project_label: metadata.project_label.clone(),
        };
        let layout =
            WorkflowRootLayout::install_scoped(project_dir.clone(), "active-listener.lock.json");
        let paths = ListenProjectPaths {
            layout: layout.clone(),
            projects_root: data_root.join("listen").join("projects"),
            project_dir: project_dir.clone(),
            project_metadata_path: metadata_path,
            state_path: layout.path("session.json"),
            lock_path: layout.active_session_path().to_path_buf(),
            logs_dir: layout.path("logs"),
            details_dir: layout.path("session-details"),
            verification_dir: layout.path("verification"),
        };
        Ok(Self { identity, paths })
    }

    pub(super) fn identity(&self) -> &ListenProjectIdentity {
        &self.identity
    }

    pub(super) fn paths(&self) -> &ListenProjectPaths {
        &self.paths
    }

    pub(super) fn ensure_layout(&self) -> Result<()> {
        ensure_dir(&self.paths.projects_root)?;
        ensure_dir(&self.paths.project_dir)?;
        ensure_dir(&self.paths.logs_dir)?;
        ensure_dir(&self.paths.details_dir)?;
        ensure_dir(&self.paths.verification_dir)?;
        self.save_metadata()
    }

    pub(super) fn save_metadata(&self) -> Result<()> {
        write_json(
            &self.paths.project_metadata_path,
            &ListenProjectMetadata {
                version: LISTEN_STORE_VERSION,
                project_key: self.identity.project_key.clone(),
                project_selector: self.identity.project_selector.clone(),
                project_label: self.identity.project_label.clone(),
                source_root: self.identity.source_root.display().to_string(),
                metastack_root: self.identity.metastack_root.display().to_string(),
                source_label: self.identity.source_label.clone(),
            },
        )
    }

    pub(super) fn load_state(&self) -> Result<ListenState> {
        let (mut state, state_exists) = self.load_state_from_disk()?;
        let repaired = self.repair_state(&mut state)?;
        let pruned = state.prune_completed_sessions_older_than(
            now_epoch_seconds(),
            COMPLETED_SESSION_TTL_SECONDS,
        );
        if state_exists && (repaired || !pruned.is_empty()) {
            self.save_state(&state)?;
        }
        Ok(state)
    }

    pub(super) fn save_state(&self, state: &ListenState) -> Result<()> {
        self.ensure_layout()?;
        write_json(&self.paths.state_path, state)?;
        self.remove_orphaned_session_details(state)?;
        for session in &state.sessions {
            self.refresh_session_detail(session)?;
        }
        Ok(())
    }

    pub(super) fn upsert_session(&self, session: AgentSession) -> Result<()> {
        let mut state = self.load_state()?;
        state.upsert(session);
        self.save_state(&state)
    }

    pub(super) fn retry_blocked_session(&self, identifier: &str) -> Result<bool> {
        let mut state = self.load_state()?;
        let session = state
            .sessions
            .iter_mut()
            .find(|s| s.issue_matches(identifier) && s.phase == SessionPhase::Blocked);
        let Some(session) = session else {
            return Ok(false);
        };
        session.phase = SessionPhase::BriefReady;
        session.blocked = None;
        session.pid = None;
        if session.latest_stale_worker_failure.is_some() {
            session.started_at_epoch_seconds = now_epoch_seconds();
            session.stale_worker_recovery_attempt_count = 0;
            session.latest_stale_worker_failure = None;
        }
        session.summary = "Retrying from previous workspace state".to_string();
        session.updated_at_epoch_seconds = now_epoch_seconds();
        self.save_state(&state)?;
        Ok(true)
    }

    pub(super) fn pause_running_session(&self, identifier: &str) -> Result<bool> {
        let mut state = self.load_state()?;
        let session = state
            .sessions
            .iter_mut()
            .find(|s| s.issue_matches(identifier) && s.phase == SessionPhase::Running);
        let Some(session) = session else {
            return Ok(false);
        };
        let Some(pid) = session.pid else {
            return Ok(false);
        };
        if !pid_is_running(pid) {
            return Ok(false);
        }
        send_process_signal(pid, ProcessSignal::Pause)?;
        session.phase = SessionPhase::Paused;
        session.blocked = None;
        session.summary = compact_session_summary([
            Some("Paused by operator".to_string()),
            Some(format!("pid {pid}")),
            session
                .backlog_issue_identifier
                .as_ref()
                .map(|identifier| format!("backlog {identifier}")),
        ]);
        session.updated_at_epoch_seconds = now_epoch_seconds();
        self.save_state(&state)?;
        Ok(true)
    }

    pub(super) fn resume_paused_session(&self, identifier: &str) -> Result<bool> {
        let mut state = self.load_state()?;
        let session = state
            .sessions
            .iter_mut()
            .find(|s| s.issue_matches(identifier) && s.phase == SessionPhase::Paused);
        let Some(session) = session else {
            return Ok(false);
        };
        let Some(pid) = session.pid else {
            return Ok(false);
        };
        if !pid_is_running(pid) {
            return Ok(false);
        }
        send_process_signal(pid, ProcessSignal::Resume)?;
        session.phase = SessionPhase::Running;
        session.blocked = None;
        session.summary = compact_session_summary([
            Some("Resumed by operator".to_string()),
            Some(format!("pid {pid}")),
            session
                .backlog_issue_identifier
                .as_ref()
                .map(|identifier| format!("backlog {identifier}")),
        ]);
        session.updated_at_epoch_seconds = now_epoch_seconds();
        self.save_state(&state)?;
        Ok(true)
    }

    pub(super) fn clear_sessions(&self, selector: &SessionSelector) -> Result<SessionClearOutcome> {
        let mut state = self.load_state()?;
        let live_sessions = state
            .sessions
            .iter()
            .filter(|session| selector.matches(session))
            .filter_map(|session| {
                session
                    .pid
                    .filter(|pid| pid_is_running(*pid))
                    .map(|pid| (session.issue_identifier.clone(), pid))
            })
            .collect::<Vec<_>>();
        if !live_sessions.is_empty() {
            let sessions = live_sessions
                .into_iter()
                .map(|(identifier, pid)| format!("{identifier} (pid {pid})"))
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "cannot clear live MetaListen session record(s) matched by {}: {}",
                selector.display_label(),
                sessions
            );
        }

        let cleared_sessions = state.remove_sessions(|session| selector.matches(session));
        if !cleared_sessions.is_empty() {
            self.save_state(&state)?;
        }

        Ok(SessionClearOutcome {
            cleared_sessions,
            remaining_sessions: state.sessions.len(),
        })
    }

    pub(super) fn log_path(&self, issue_identifier: &str) -> PathBuf {
        self.paths.logs_dir.join(format!("{issue_identifier}.log"))
    }

    pub(super) fn detail_path(&self, issue_identifier: &str) -> PathBuf {
        self.paths
            .details_dir
            .join(format!("{issue_identifier}.json"))
    }

    pub(super) fn verification_json_path(&self, issue_identifier: &str) -> PathBuf {
        self.paths
            .verification_dir
            .join(format!("{issue_identifier}.json"))
    }

    pub(super) fn verification_markdown_path(&self, issue_identifier: &str) -> PathBuf {
        self.paths
            .verification_dir
            .join(format!("{issue_identifier}.md"))
    }

    pub(super) fn load_session_detail(
        &self,
        issue_identifier: &str,
    ) -> Result<Option<ListenSessionDetail>> {
        read_optional_json_lossy(&self.detail_path(issue_identifier))
    }

    pub(super) fn load_verification_report(
        &self,
        issue_identifier: &str,
    ) -> Result<Option<VerificationReport>> {
        read_optional_json_lossy(&self.verification_json_path(issue_identifier))
    }

    pub(super) fn write_verification_report(
        &self,
        issue_identifier: &str,
        report: &VerificationReport,
    ) -> Result<()> {
        self.ensure_layout()?;
        write_json(&self.verification_json_path(issue_identifier), report)?;
        fs::write(
            self.verification_markdown_path(issue_identifier),
            report.render_markdown(),
        )
        .with_context(|| {
            format!(
                "failed to write `{}`",
                self.verification_markdown_path(issue_identifier).display()
            )
        })?;
        Ok(())
    }

    pub(super) fn load_session_details(
        &self,
        app_config: &AppConfig,
        sessions: &[AgentSession],
    ) -> Result<Vec<ListenSessionDetail>> {
        let mut details = Vec::new();
        for session in sessions {
            if let Some(mut detail) = self.load_session_detail(&session.issue_identifier)? {
                if detail.context_budget_tokens.is_none() {
                    detail.context_budget_tokens =
                        Some(resolve_session_context_budget_tokens(app_config, session));
                }
                details.push(detail);
            }
        }
        Ok(details)
    }

    pub(super) fn acquire_listener_lock(&self, pid: u32) -> Result<ListenerLockGuard> {
        self.ensure_layout()?;
        let active_lock_file = self.active_lock_file();

        loop {
            match self.inspect_active_lock()? {
                ActiveLockInspection::Missing => {}
                ActiveLockInspection::Present(existing) => {
                    if pid_is_running(existing.lock.pid) {
                        bail!(
                            "another `{} listen` instance already owns project `{}` (pid {}); active lock: {}",
                            crate::branding::COMMAND_NAME,
                            self.identity.project_label,
                            existing.lock.pid,
                            self.paths.lock_path.display()
                        );
                    }

                    eprintln!(
                        "warning: removing stale active listener lock for project `{}` at {} (pid {} is not running)",
                        self.identity.project_label,
                        self.paths.lock_path.display(),
                        existing.lock.pid
                    );
                    let _ = remove_file_if_identity_matches(
                        &self.paths.lock_path,
                        existing.identity.as_ref(),
                    )?;
                    continue;
                }
                ActiveLockInspection::Invalid { identity, error } => {
                    if let Some((recovered, _candidate)) =
                        first_valid_recovery_candidate::<ActiveListenerLock>(
                            &recovery_candidate_paths(&self.paths.lock_path)?,
                        )
                    {
                        if pid_is_running(recovered.pid) {
                            bail!(
                                "another `{} listen` instance already owns project `{}` (pid {}); active lock: {}",
                                crate::branding::COMMAND_NAME,
                                self.identity.project_label,
                                recovered.pid,
                                self.paths.lock_path.display()
                            );
                        }
                    }

                    eprintln!(
                        "warning: removing unreadable active listener lock for project `{}` at {}: {error:#}",
                        self.identity.project_label,
                        self.paths.lock_path.display()
                    );
                    let _ =
                        remove_file_if_identity_matches(&self.paths.lock_path, identity.as_ref())?;
                    continue;
                }
            }

            let lock = ActiveListenerLock {
                pid,
                acquired_at_epoch_seconds: now_epoch_seconds(),
                source_root: self.identity.source_root.display().to_string(),
                metastack_root: self.identity.metastack_root.display().to_string(),
            };
            match active_lock_file.try_create_new(&lock)? {
                true => {
                    return Ok(ListenerLockGuard {
                        lock_path: self.paths.lock_path.clone(),
                        project_label: self.identity.project_label.clone(),
                        identity: read_file_identity(&self.paths.lock_path).unwrap_or_else(
                            |error| {
                                eprintln!(
                                    "warning: failed to capture active listener lock identity for project `{}` at {}: {error:#}",
                                    self.identity.project_label,
                                    self.paths.lock_path.display()
                                );
                                None
                            },
                        ),
                    });
                }
                false => continue,
            }
        }
    }

    pub(super) fn load_active_lock(&self) -> Result<Option<ActiveListenerLock>> {
        match self.inspect_active_lock()? {
            ActiveLockInspection::Missing => Ok(None),
            ActiveLockInspection::Present(existing) => Ok(Some(existing.lock)),
            ActiveLockInspection::Invalid { error, .. } => Err(error),
        }
    }

    fn inspect_active_lock(&self) -> Result<ActiveLockInspection> {
        let identity = match read_file_identity(&self.paths.lock_path) {
            Ok(identity) => identity,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(ActiveLockInspection::Missing);
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to inspect `{}`", self.paths.lock_path.display())
                });
            }
        };

        match read_required_json_with_recovery::<ActiveListenerLock>(
            &self.paths.lock_path,
            &self.identity.project_label,
            "active listener lock",
        ) {
            Ok(lock) => Ok(ActiveLockInspection::Present(LockSnapshot {
                lock,
                identity: read_file_identity(&self.paths.lock_path).unwrap_or(identity),
            })),
            Err(error) if is_not_found_error(&error) => Ok(ActiveLockInspection::Missing),
            Err(error) => Ok(ActiveLockInspection::Invalid { identity, error }),
        }
    }

    /// Removes the stored session entry, structured detail artifact, and per-ticket log file for
    /// one Linear ticket.
    ///
    /// Returns an error when the persisted state cannot be read or updated, or when the matching
    /// detail/log files cannot be removed.
    pub(crate) fn remove_ticket_artifacts(&self, issue_identifier: &str) -> Result<()> {
        let mut state = self.load_state()?;
        if state.remove_issue(issue_identifier) {
            self.save_state(&state)?;
        }

        let detail_path = self.detail_path(issue_identifier);
        match fs::remove_file(&detail_path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to remove `{}`", detail_path.display()));
            }
        }

        let log_path = self.log_path(issue_identifier);
        match fs::remove_file(&log_path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to remove `{}`", log_path.display()));
            }
        }

        let verification_json_path = self.verification_json_path(issue_identifier);
        match fs::remove_file(&verification_json_path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to remove `{}`", verification_json_path.display())
                });
            }
        }

        let verification_markdown_path = self.verification_markdown_path(issue_identifier);
        match fs::remove_file(&verification_markdown_path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to remove `{}`",
                        verification_markdown_path.display()
                    )
                });
            }
        }

        Ok(())
    }

    pub(super) fn list_projects() -> Result<Vec<StoredListenProjectSummary>> {
        let data_root = resolve_data_root()?;
        Self::list_projects_with_data_root(data_root)
    }

    fn list_projects_with_data_root(data_root: PathBuf) -> Result<Vec<StoredListenProjectSummary>> {
        let projects_root = data_root.join("listen").join("projects");
        let mut projects = Vec::new();

        let entries = match fs::read_dir(&projects_root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(projects),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to read `{}`", projects_root.display()));
            }
        };

        for entry in entries {
            let entry =
                entry.with_context(|| format!("failed to read `{}`", projects_root.display()))?;
            if !entry
                .file_type()
                .with_context(|| format!("failed to inspect `{}`", entry.path().display()))?
                .is_dir()
            {
                continue;
            }

            let project_dir = entry.path();
            let metadata_path = project_dir.join("project.json");
            let state_path = project_dir.join("session.json");
            let lock_path = project_dir.join("active-listener.lock.json");
            let logs_dir = project_dir.join("logs");
            let details_dir = project_dir.join("session-details");
            let metadata = match read_required_json_with_recovery::<ListenProjectMetadata>(
                &metadata_path,
                project_dir
                    .file_name()
                    .and_then(OsStr::to_str)
                    .unwrap_or("project"),
                "listen project metadata",
            ) {
                Ok(metadata) => metadata,
                Err(error) => {
                    eprintln!(
                        "warning: skipping unreadable listen project metadata at {}: {error:#}",
                        metadata_path.display()
                    );
                    continue;
                }
            };
            let store = Self {
                identity: ListenProjectIdentity {
                    project_key: metadata.project_key.clone(),
                    source_root: PathBuf::from(&metadata.source_root),
                    metastack_root: PathBuf::from(&metadata.metastack_root),
                    source_label: metadata.source_label.clone(),
                    project_selector: metadata.project_selector.clone(),
                    project_label: metadata.project_label.clone(),
                },
                paths: ListenProjectPaths {
                    layout: WorkflowRootLayout::install_scoped(
                        project_dir.clone(),
                        "active-listener.lock.json",
                    ),
                    projects_root: projects_root.clone(),
                    project_dir: project_dir.clone(),
                    project_metadata_path: metadata_path.clone(),
                    state_path: state_path.clone(),
                    lock_path: lock_path.clone(),
                    logs_dir: logs_dir.clone(),
                    details_dir,
                    verification_dir: project_dir.join("verification"),
                },
            };
            let latest_session = match store.load_state() {
                Ok(state) => state.latest_session(),
                Err(_) => None,
            };
            let active_lock = store.load_active_lock().ok().flatten();

            projects.push(StoredListenProjectSummary {
                metadata,
                state_path,
                lock_path,
                logs_dir,
                latest_session,
                active_lock,
            });
        }

        projects.sort_by(|left, right| {
            right
                .latest_session
                .as_ref()
                .map(|session| session.updated_at_epoch_seconds)
                .unwrap_or_default()
                .cmp(
                    &left
                        .latest_session
                        .as_ref()
                        .map(|session| session.updated_at_epoch_seconds)
                        .unwrap_or_default(),
                )
                .then_with(|| {
                    left.metadata
                        .project_label
                        .cmp(&right.metadata.project_label)
                })
        });
        Ok(projects)
    }

    fn load_state_from_disk(&self) -> Result<(ListenState, bool)> {
        match read_required_json_with_recovery::<ListenState>(
            &self.paths.state_path,
            &self.identity.project_label,
            "listen state",
        ) {
            Ok(state) => Ok((state, true)),
            Err(error) if is_not_found_error(&error) => Ok((ListenState::default(), false)),
            Err(error) => Err(error),
        }
    }

    fn refresh_session_detail(&self, session: &AgentSession) -> Result<()> {
        let path = self.detail_path(&session.issue_identifier);
        let mut detail = self
            .load_session_detail(&session.issue_identifier)?
            .unwrap_or_else(|| ListenSessionDetail {
                version: LISTEN_SESSION_DETAIL_VERSION,
                issue_identifier: session.issue_identifier.clone(),
                issue_title: session.issue_title.clone(),
                started_at_epoch_seconds: session.started_at_epoch_seconds,
                updated_at_epoch_seconds: session.updated_at_epoch_seconds,
                session_updated_at_epoch_seconds: session.updated_at_epoch_seconds,
                phase: session.phase,
                summary: session.summary.clone(),
                blocked: session.blocked.clone(),
                turns: session.turns,
                tokens: session.tokens.clone(),
                turn_history: session.turn_history.clone(),
                context_budget_tokens: session.context_budget_tokens,
                canonical: session.canonical.clone(),
                pull_request: session.pull_request.clone(),
                latest_resume_handle: session.latest_resume_handle.clone(),
                verification: None,
                pending_linear_sync: session.pending_linear_sync.clone(),
                stale_worker_recovery_attempt_count: session.stale_worker_recovery_attempt_count,
                latest_stale_worker_failure: session.latest_stale_worker_failure.clone(),
                last_timeout: session.last_timeout.clone(),
                references: SessionDetailReferences::default(),
                prompt_context: Vec::new(),
                milestones: Vec::new(),
                log_excerpts: Vec::new(),
            });

        detail.version = LISTEN_SESSION_DETAIL_VERSION;
        detail.issue_identifier = session.issue_identifier.clone();
        detail.issue_title = session.issue_title.clone();
        detail.started_at_epoch_seconds = session.started_at_epoch_seconds;
        detail.updated_at_epoch_seconds = now_epoch_seconds();
        detail.session_updated_at_epoch_seconds = session.updated_at_epoch_seconds;
        detail.phase = session.phase;
        detail.summary = session.summary.clone();
        detail.blocked = session.blocked.clone();
        detail.turns = session.turns;
        detail.tokens = session.tokens.clone();
        detail.turn_history = session.turn_history.clone();
        detail.context_budget_tokens = session
            .context_budget_tokens
            .or(detail.context_budget_tokens);
        detail.canonical = session.canonical.clone();
        detail.pull_request = session.pull_request.clone();
        detail.latest_resume_handle = session.latest_resume_handle.clone();
        detail.verification = self
            .load_verification_report(&session.issue_identifier)?
            .map(|report| report.summary_snapshot());
        detail.pending_linear_sync = session.pending_linear_sync.clone();
        detail.stale_worker_recovery_attempt_count = session.stale_worker_recovery_attempt_count;
        detail.latest_stale_worker_failure = session.latest_stale_worker_failure.clone();
        detail.last_timeout = session.last_timeout.clone();
        detail.references = SessionDetailReferences {
            workspace_path: session.workspace_path.clone(),
            backlog_path: session.backlog_path.clone(),
            brief_path: session.brief_path.clone(),
            workpad_comment_id: session.workpad_comment_id.clone(),
            log_path: session.log_path.clone(),
            branch: session.branch.clone(),
            verification_json_path: self
                .verification_json_path(&session.issue_identifier)
                .is_file()
                .then(|| {
                    self.verification_json_path(&session.issue_identifier)
                        .display()
                        .to_string()
                }),
            verification_markdown_path: self
                .verification_markdown_path(&session.issue_identifier)
                .is_file()
                .then(|| {
                    self.verification_markdown_path(&session.issue_identifier)
                        .display()
                        .to_string()
                }),
        };
        detail.prompt_context = build_prompt_context_references(session);
        detail.log_excerpts = read_log_excerpts(session.log_path.as_deref())?;
        append_milestone(&mut detail.milestones, session);

        write_json(&path, &detail)
    }

    fn repair_state(&self, state: &mut ListenState) -> Result<bool> {
        let mut changed = false;
        for session in &mut state.sessions {
            let detail = self.load_session_detail(&session.issue_identifier)?;
            changed |= repair_session(self, session, detail.as_ref())?;
        }
        Ok(changed)
    }

    fn remove_orphaned_session_details(&self, state: &ListenState) -> Result<()> {
        let valid = state
            .sessions
            .iter()
            .map(|session| session.issue_identifier.to_ascii_lowercase())
            .collect::<std::collections::BTreeSet<_>>();
        let entries = match fs::read_dir(&self.paths.details_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to read `{}`", self.paths.details_dir.display())
                });
            }
        };

        for entry in entries {
            let entry = entry.with_context(|| {
                format!("failed to read `{}`", self.paths.details_dir.display())
            })?;
            if !entry
                .file_type()
                .with_context(|| format!("failed to inspect `{}`", entry.path().display()))?
                .is_file()
            {
                continue;
            }
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(OsStr::to_str) else {
                continue;
            };
            if valid.contains(&stem.to_ascii_lowercase()) {
                continue;
            }
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove `{}`", path.display()))?;
            for verification_path in [
                self.verification_json_path(stem),
                self.verification_markdown_path(stem),
            ] {
                match fs::remove_file(&verification_path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(error).with_context(|| {
                            format!("failed to remove `{}`", verification_path.display())
                        });
                    }
                }
            }
        }

        Ok(())
    }

    fn active_lock_file(&self) -> ActiveSessionFile<ActiveListenerLock> {
        self.paths.layout.active_session_file()
    }
}

fn resolve_session_context_budget_tokens(app_config: &AppConfig, session: &AgentSession) -> u64 {
    session
        .context_budget_tokens
        .or_else(|| {
            session
                .workspace_path
                .as_deref()
                .and_then(|workspace_path| PlanningMeta::load(Path::new(workspace_path)).ok())
                .map(|planning_meta| {
                    planning_meta.effective_listen_context_budget_tokens(app_config)
                })
        })
        .unwrap_or_else(|| app_config.defaults.listen.context_budget_tokens())
}

impl Drop for ListenerLockGuard {
    fn drop(&mut self) {
        match remove_file_if_identity_matches(&self.lock_path, self.identity.as_ref()) {
            Ok(ConditionalRemoveOutcome::Removed | ConditionalRemoveOutcome::Missing) => {}
            Ok(ConditionalRemoveOutcome::Replaced) => {
                eprintln!(
                    "warning: skipping active listener lock cleanup for project `{}` at {} because the lock file was replaced",
                    self.project_label,
                    self.lock_path.display()
                );
            }
            Err(error) => {
                eprintln!(
                    "warning: failed to clean up active listener lock for project `{}` at {}: {error:#}",
                    self.project_label,
                    self.lock_path.display()
                );
            }
        }
    }
}

impl FileIdentity {
    #[cfg(unix)]
    fn from_metadata(metadata: &fs::Metadata) -> Option<Self> {
        Some(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    #[cfg(not(unix))]
    fn from_metadata(_metadata: &fs::Metadata) -> Option<Self> {
        None
    }
}

fn read_required_json_with_recovery<T>(
    path: &Path,
    project_context: &str,
    record_label: &str,
) -> Result<T>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    match read_json(path) {
        Ok(value) => Ok(value),
        Err(error) if is_not_found_error(&error) || !is_json_decode_error(&error) => Err(error),
        Err(primary_error) => {
            recover_required_json(path, project_context, record_label, primary_error)
        }
    }
}

fn recover_required_json<T>(
    path: &Path,
    project_context: &str,
    record_label: &str,
    primary_error: anyhow::Error,
) -> Result<T>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    let candidates = recovery_candidate_paths(path)?;
    let mut attempted = Vec::with_capacity(candidates.len());
    attempted.extend(
        candidates
            .iter()
            .map(|candidate| candidate.display().to_string()),
    );

    if let Some((recovered, candidate)) = first_valid_recovery_candidate::<T>(&candidates) {
        write_json(path, &recovered)
            .with_context(|| format!("failed to rewrite recovered `{}`", path.display()))?;
        cleanup_recovery_artifact(&candidate, path, project_context, record_label);
        eprintln!(
            "warning: recovered {record_label} for project `{project_context}` at {} from {}",
            path.display(),
            candidate.display()
        );
        return Ok(recovered);
    }

    let attempted = if attempted.is_empty() {
        "none".to_string()
    } else {
        attempted.join(", ")
    };
    bail!(
        "failed to recover corrupted {record_label} for project `{project_context}` at `{}`; attempted recovery paths: {attempted}; primary error: {primary_error:#}",
        path.display()
    );
}

fn cleanup_recovery_artifact(
    candidate: &Path,
    path: &Path,
    project_context: &str,
    record_label: &str,
) {
    match fs::remove_file(candidate) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            eprintln!(
                "warning: recovered {record_label} for project `{project_context}` at {} from {} but failed to remove the consumed recovery artifact: {error:#}",
                path.display(),
                candidate.display()
            );
        }
    }
}

fn recovery_candidate_paths(path: &Path) -> Result<Vec<PathBuf>> {
    let mut temp_siblings = BTreeSet::new();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("state.json");
    let bak_path = sibling_recovery_path(path, "bak");
    let tmp_path = sibling_recovery_path(path, "tmp");

    match fs::read_dir(parent) {
        Ok(entries) => {
            let prefix = format!("{file_name}.");
            for entry in entries {
                let entry =
                    entry.with_context(|| format!("failed to read `{}`", parent.display()))?;
                let candidate_path = entry.path();
                let Some(candidate_name) = candidate_path.file_name().and_then(OsStr::to_str)
                else {
                    continue;
                };
                if candidate_name.starts_with(&prefix) && candidate_name.ends_with(".tmp") {
                    temp_siblings.insert(candidate_path);
                }
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read `{}`", parent.display()));
        }
    }

    temp_siblings.remove(&tmp_path);

    let mut candidates = vec![bak_path, tmp_path];
    candidates.extend(temp_siblings);
    Ok(candidates)
}

fn first_valid_recovery_candidate<T>(candidates: &[PathBuf]) -> Option<(T, PathBuf)>
where
    T: for<'de> Deserialize<'de>,
{
    for candidate in candidates {
        let contents = match fs::read_to_string(candidate) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(_) => continue,
        };
        let recovered = match serde_json::from_str::<T>(&contents) {
            Ok(value) => value,
            Err(_) => continue,
        };
        return Some((recovered, candidate.clone()));
    }
    None
}

fn sibling_recovery_path(path: &Path, suffix: &str) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("state.json");
    path.with_file_name(format!("{file_name}.{suffix}"))
}

fn is_json_decode_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.is::<serde_json::Error>())
}

fn read_file_identity(path: &Path) -> std::io::Result<Option<FileIdentity>> {
    fs::metadata(path).map(|metadata| FileIdentity::from_metadata(&metadata))
}

fn remove_file_if_identity_matches(
    path: &Path,
    expected_identity: Option<&FileIdentity>,
) -> Result<ConditionalRemoveOutcome> {
    if let Some(expected_identity) = expected_identity {
        let actual_identity = match read_file_identity(path) {
            Ok(Some(identity)) => Some(identity),
            Ok(None) => None,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(ConditionalRemoveOutcome::Missing);
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to inspect `{}`", path.display()));
            }
        };
        if actual_identity.as_ref() != Some(expected_identity) {
            return Ok(ConditionalRemoveOutcome::Replaced);
        }
    }

    match fs::remove_file(path) {
        Ok(()) => Ok(ConditionalRemoveOutcome::Removed),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(ConditionalRemoveOutcome::Missing),
        Err(error) => Err(error).with_context(|| format!("failed to remove `{}`", path.display())),
    }
}

fn resolve_project_identity(
    root: &Path,
    project_selector: Option<&str>,
) -> Result<ListenProjectIdentity> {
    let requested_root = canonicalize_existing_dir(root)?;
    let source_root = resolve_source_root(&requested_root)?;
    let metastack_root =
        canonicalize_existing_dir(&source_root.join(crate::branding::PROJECT_DIR))?;
    let source_label = source_root
        .file_name()
        .and_then(OsStr::to_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("project")
        .to_string();
    let project_selector = normalize_project_selector(project_selector);
    let project_label = project_selector
        .clone()
        .unwrap_or_else(|| "All projects".to_string());

    Ok(ListenProjectIdentity {
        project_key: project_key_for_metastack_root(&metastack_root, project_selector.as_deref()),
        source_root,
        metastack_root,
        source_label,
        project_selector,
        project_label,
    })
}

/// Resolves the source repository root for a requested path, collapsing git worktrees back to the
/// owning repository when the shared project directory (see `crate::branding::PROJECT_DIR`) lives there.
///
/// Returns an error when the requested path cannot be resolved.
pub(crate) fn resolve_source_project_root(root: &Path) -> Result<PathBuf> {
    let requested_root = canonicalize_existing_dir(root)?;
    resolve_source_root(&requested_root)
}

fn resolve_source_root(root: &Path) -> Result<PathBuf> {
    let common_dir = git_stdout(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    );
    if let Ok(common_dir) = common_dir {
        let common_dir = PathBuf::from(common_dir);
        if common_dir.file_name() == Some(OsStr::new(".git"))
            && let Some(source_root) = common_dir.parent()
            && source_root.join(crate::branding::PROJECT_DIR).is_dir()
        {
            return canonicalize_existing_dir(source_root);
        }
    }

    Ok(root.to_path_buf())
}

fn project_key_for_metastack_root(metastack_root: &Path, project_selector: Option<&str>) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    metastack_root.display().to_string().hash(&mut hasher);
    normalized_project_scope_key(project_selector).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn normalize_project_selector(project_selector: Option<&str>) -> Option<String> {
    project_selector
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn normalized_project_scope_key(project_selector: Option<&str>) -> String {
    match normalize_project_selector(project_selector) {
        Some(selector) => format!("project:{}", selector.to_ascii_lowercase()),
        None => "project:all".to_string(),
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

fn is_not_found_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
        .any(|io_error| io_error.kind() == ErrorKind::NotFound)
}

fn append_milestone(milestones: &mut Vec<SessionMilestone>, session: &AgentSession) {
    let candidate = SessionMilestone {
        at_epoch_seconds: session.updated_at_epoch_seconds,
        phase: session.phase,
        summary: session.summary.clone(),
        turns: session.turns,
        pull_request_status: session.pull_request.status,
        pull_request_number: session.pull_request.number,
    };
    if milestones.last().is_some_and(|last| {
        last.phase == candidate.phase
            && last.summary == candidate.summary
            && last.turns == candidate.turns
            && last.pull_request_status == candidate.pull_request_status
            && last.pull_request_number == candidate.pull_request_number
    }) {
        return;
    }
    milestones.push(candidate);
}

fn build_prompt_context_references(session: &AgentSession) -> Vec<SessionContextReference> {
    let mut references = Vec::new();

    if let Some(path) = session.brief_path.as_ref() {
        references.push(SessionContextReference {
            label: "Brief".to_string(),
            value: path.clone(),
        });
    }
    if let Some(path) = session.backlog_path.as_ref() {
        references.push(SessionContextReference {
            label: "Backlog".to_string(),
            value: path.clone(),
        });
    }
    if let Some(workspace_path) = session.workspace_path.as_deref() {
        let paths = PlanningPaths::new(Path::new(workspace_path));
        let issue_identifier = session
            .backlog_issue_identifier
            .as_deref()
            .unwrap_or(&session.issue_identifier);
        let backlog_index = paths.backlog_issue_dir(issue_identifier).join("index.md");
        if backlog_index.is_file() {
            references.push(SessionContextReference {
                label: "Backlog index".to_string(),
                value: backlog_index.display().to_string(),
            });
        }

        let manifest_path = paths.agent_issue_context_manifest_path(&session.issue_identifier);
        if manifest_path.is_file() {
            references.push(SessionContextReference {
                label: "Attachment context manifest".to_string(),
                value: manifest_path.display().to_string(),
            });
        }
    }

    references
}

fn repair_session(
    store: &ListenProjectStore,
    session: &mut AgentSession,
    detail: Option<&ListenSessionDetail>,
) -> Result<bool> {
    let original_started_at = session.started_at_epoch_seconds;
    let original_recovery_attempt_count = session.stale_worker_recovery_attempt_count;
    let original_latest_stale_worker_failure = session.latest_stale_worker_failure.clone();
    let original_tokens = session.tokens.clone();
    let original_turn_history = session.turn_history.clone();
    let original_canonical = session.canonical.clone();
    let log_path = resolve_store_path(
        store,
        session
            .log_path
            .as_deref()
            .or_else(|| detail.and_then(|value| value.references.log_path.as_deref())),
    );
    let log_recovery = repair_from_worker_log(log_path.as_deref())?;
    let mut recovered_sources = Vec::new();
    let mut skip_notes = log_recovery.notes;

    if session.turn_history.is_empty()
        && let Some(turn_history) = detail
            .map(|value| value.turn_history.clone())
            .filter(|turn_history| !turn_history.is_empty())
    {
        session.turn_history = turn_history;
    }

    if session.started_at_epoch_seconds == 0 {
        session.started_at_epoch_seconds = detail
            .map(|value| value.started_at_epoch_seconds)
            .filter(|started_at| *started_at > 0)
            .unwrap_or(session.updated_at_epoch_seconds);
    }

    if session.stale_worker_recovery_attempt_count == 0
        && let Some(recovery_attempt_count) = detail
            .map(|value| value.stale_worker_recovery_attempt_count)
            .filter(|count| *count > 0)
    {
        session.stale_worker_recovery_attempt_count = recovery_attempt_count;
    }

    if session.latest_stale_worker_failure.is_none()
        && let Some(latest_failure) =
            detail.and_then(|value| value.latest_stale_worker_failure.clone())
    {
        session.latest_stale_worker_failure = Some(latest_failure);
    }

    if session.canonical.provider.is_none() {
        if let Some(provider) = detail.and_then(|value| value.canonical.provider.clone()) {
            session.canonical.provider = Some(provider);
            recovered_sources.push("detail_provider".to_string());
        } else if let Some(provider) = session
            .latest_resume_handle
            .as_ref()
            .map(|resume| resume.provider.label().to_string())
        {
            session.canonical.provider = Some(provider);
            recovered_sources.push("resume_provider".to_string());
        } else if let Some(provider) = detail
            .and_then(|value| value.latest_resume_handle.as_ref())
            .map(|resume| resume.provider.label().to_string())
        {
            session.canonical.provider = Some(provider);
            recovered_sources.push("detail_resume_provider".to_string());
        } else if let Some(provider) = log_recovery.provider {
            session.canonical.provider = Some(provider);
            recovered_sources.push("worker_log_provider".to_string());
        }
    }

    if session.canonical.model.is_none() {
        if let Some(model) = detail.and_then(|value| value.canonical.model.clone()) {
            session.canonical.model = Some(model);
            recovered_sources.push("detail_model".to_string());
        } else if let Some(model) = log_recovery.model {
            session.canonical.model = Some(model);
            recovered_sources.push("worker_log_model".to_string());
        }
    }

    if session.canonical.reasoning.is_none() {
        if let Some(reasoning) = detail.and_then(|value| value.canonical.reasoning.clone()) {
            session.canonical.reasoning = Some(reasoning);
            recovered_sources.push("detail_reasoning".to_string());
        } else if let Some(reasoning) = log_recovery.reasoning {
            session.canonical.reasoning = Some(reasoning);
            recovered_sources.push("worker_log_reasoning".to_string());
        }
    }

    if !session.canonical.tokens.is_known() {
        if detail
            .map(|value| value.canonical.tokens.is_known())
            .unwrap_or(false)
        {
            session.canonical.tokens = detail
                .map(|value| value.canonical.tokens.clone())
                .unwrap_or_default();
            recovered_sources.push("detail_canonical_tokens".to_string());
        } else if session.tokens.is_known() {
            session.canonical.tokens = session.tokens.clone();
            recovered_sources.push("legacy_state_tokens".to_string());
        } else if detail.map(|value| value.tokens.is_known()).unwrap_or(false) {
            session.canonical.tokens = detail.map(|value| value.tokens.clone()).unwrap_or_default();
            recovered_sources.push("detail_tokens".to_string());
        } else if log_recovery.tokens.is_known() {
            session.canonical.tokens = log_recovery.tokens;
            recovered_sources.push("worker_log_tokens".to_string());
        } else {
            skip_notes.push("no recoverable token evidence".to_string());
        }
    }

    if session.canonical.tokens.is_known() && session.tokens != session.canonical.tokens {
        session.tokens = session.canonical.tokens.clone();
    }

    if !recovered_sources.is_empty() {
        session.canonical.repair = Some(CanonicalRepairRecord {
            status: CanonicalRepairStatus::Recovered,
            source: Some(recovered_sources.join(",")),
            note: None,
        });
    } else if original_canonical.provider.is_none()
        && original_canonical.model.is_none()
        && original_canonical.reasoning.is_none()
        && !original_canonical.tokens.is_known()
        && (!session.canonical.tokens.is_known() || session.canonical.provider.is_none())
    {
        session.canonical.repair = Some(CanonicalRepairRecord {
            status: CanonicalRepairStatus::Skipped,
            source: Some("local_state,detail,worker_log".to_string()),
            note: Some(compact_repair_note(&skip_notes)),
        });
    }

    Ok(session.started_at_epoch_seconds != original_started_at
        || session.stale_worker_recovery_attempt_count != original_recovery_attempt_count
        || session.latest_stale_worker_failure != original_latest_stale_worker_failure
        || session.tokens != original_tokens
        || session.turn_history != original_turn_history
        || session.canonical != original_canonical)
}

fn resolve_store_path(store: &ListenProjectStore, raw_path: Option<&str>) -> Option<PathBuf> {
    let raw_path = raw_path?.trim();
    if raw_path.is_empty() {
        return None;
    }

    let candidate = PathBuf::from(raw_path);
    if candidate.is_absolute() {
        return Some(candidate);
    }

    let project_path = store.paths.project_dir.join(&candidate);
    if project_path.exists() {
        return Some(project_path);
    }

    let source_path = store.identity.source_root.join(&candidate);
    if source_path.exists() {
        return Some(source_path);
    }

    Some(project_path)
}

#[derive(Debug, Default)]
struct WorkerLogRecovery {
    provider: Option<String>,
    model: Option<String>,
    reasoning: Option<String>,
    tokens: TokenUsage,
    notes: Vec<String>,
}

#[derive(Debug, Default)]
struct WorkerLogTurn {
    provider: Option<String>,
    model: Option<String>,
    reasoning: Option<String>,
    contents: String,
}

fn repair_from_worker_log(path: Option<&Path>) -> Result<WorkerLogRecovery> {
    let Some(path) = path else {
        return Ok(WorkerLogRecovery::default());
    };
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(WorkerLogRecovery::default());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read `{}`", path.display()));
        }
    };

    let mut provider_values = BTreeSet::new();
    let mut model_values = BTreeSet::new();
    let mut reasoning_values = BTreeSet::new();
    let mut turns = Vec::new();
    let mut current_turn = WorkerLogTurn::default();

    // Persisted listen logs are a compatibility surface for session repair. Only the current
    // branded and legacy `meta` turn headers plus the corresponding preflight-failure headers are
    // treated as explicit historical block boundaries.
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if is_listen_turn_log_header(line) || is_listen_preflight_failure_log_header(line) {
            if !current_turn.contents.is_empty() || current_turn.provider.is_some() {
                turns.push(current_turn);
                current_turn = WorkerLogTurn::default();
            }
            continue;
        }

        if let Some(provider) = line.strip_prefix("Resolved provider: ") {
            let provider = provider.trim();
            if !provider.is_empty() {
                provider_values.insert(provider.to_string());
                current_turn.provider = Some(provider.to_string());
            }
        } else if let Some(model) = line.strip_prefix("Resolved model: ") {
            let model = model.trim();
            if !model.eq_ignore_ascii_case("unset") && !model.is_empty() {
                model_values.insert(model.to_string());
                current_turn.model = Some(model.to_string());
            }
        } else if let Some(reasoning) = line.strip_prefix("Resolved reasoning: ") {
            let reasoning = reasoning.trim();
            if !reasoning.eq_ignore_ascii_case("unset") && !reasoning.is_empty() {
                reasoning_values.insert(reasoning.to_string());
                current_turn.reasoning = Some(reasoning.to_string());
            }
        }

        current_turn.contents.push_str(raw_line);
        current_turn.contents.push('\n');
    }

    if !current_turn.contents.is_empty() || current_turn.provider.is_some() {
        turns.push(current_turn);
    }

    let mut notes = Vec::new();
    let provider = unique_value(&provider_values, "provider", &mut notes);
    let model = unique_value(&model_values, "model", &mut notes);
    let reasoning = unique_value(&reasoning_values, "reasoning", &mut notes);

    let mut recovered_tokens = TokenUsage::default();
    for turn in turns {
        let provider_name = turn.provider.as_deref().or(provider.as_deref());
        let Some(provider_name) = provider_name else {
            continue;
        };
        let Some(adapter) = builtin_provider_adapter(provider_name) else {
            continue;
        };
        let parsed = match adapter.parse_capture_output(&turn.contents) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        if let Some(usage) = parsed.usage {
            recovered_tokens.accumulate(&TokenUsage {
                input: usage.input,
                output: usage.output,
            });
        }
    }

    Ok(WorkerLogRecovery {
        provider,
        model,
        reasoning,
        tokens: recovered_tokens,
        notes,
    })
}

fn unique_value(values: &BTreeSet<String>, label: &str, notes: &mut Vec<String>) -> Option<String> {
    if values.len() > 1 {
        notes.push(format!("ambiguous {label} evidence"));
        return None;
    }
    values.iter().next().cloned()
}

fn compact_repair_note(notes: &[String]) -> String {
    if notes.is_empty() {
        "no recoverable local evidence".to_string()
    } else {
        notes.join("; ")
    }
}

fn read_log_excerpts(log_path: Option<&str>) -> Result<Vec<SessionLogExcerpt>> {
    let Some(log_path) = log_path else {
        return Ok(Vec::new());
    };
    let path = Path::new(log_path);
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read `{}`", path.display()));
        }
    };

    let lines = contents
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            summarize_log_line(line).map(|text| SessionLogExcerpt {
                line_number: index + 1,
                text,
            })
        })
        .collect::<Vec<_>>();
    let start = lines.len().saturating_sub(LOG_EXCERPT_LIMIT);
    Ok(lines.into_iter().skip(start).collect())
}

fn summarize_log_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            return Some(truncate_log_excerpt(message));
        }
        if let Some(message) = value.get("msg").and_then(Value::as_str) {
            return Some(truncate_log_excerpt(message));
        }
        if let Some(error) = value.get("error").and_then(Value::as_str) {
            return Some(truncate_log_excerpt(&format!("error: {error}")));
        }
        if let Some(kind) = value.get("type").and_then(Value::as_str) {
            let detail = value
                .get("subtype")
                .and_then(Value::as_str)
                .or_else(|| value.get("event").and_then(Value::as_str))
                .or_else(|| value.get("thread_id").and_then(Value::as_str))
                .or_else(|| value.get("session_id").and_then(Value::as_str))
                .unwrap_or_default();
            let summary = if detail.is_empty() {
                kind.to_string()
            } else {
                format!("{kind}: {detail}")
            };
            return Some(truncate_log_excerpt(&summary));
        }
    }

    Some(truncate_log_excerpt(trimmed))
}

fn truncate_log_excerpt(value: &str) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = collapsed.chars();
    let truncated = chars
        .by_ref()
        .take(LOG_EXCERPT_MAX_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        format!("{}...", truncated.trim_end())
    } else {
        collapsed
    }
}

pub(super) fn pid_is_running(pid: u32) -> bool {
    Command::new("ps")
        .arg("-p")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(true)
}

#[derive(Debug, Clone, Copy)]
enum ProcessSignal {
    Pause,
    Resume,
}

#[cfg(unix)]
fn send_process_signal(pid: u32, signal: ProcessSignal) -> Result<()> {
    let signal_arg = match signal {
        ProcessSignal::Pause => "-STOP",
        ProcessSignal::Resume => "-CONT",
    };
    let status = Command::new("kill")
        .arg(signal_arg)
        .arg(pid.to_string())
        .status()
        .with_context(|| format!("failed to run `kill {signal_arg} {pid}`"))?;
    if !status.success() {
        bail!("`kill {signal_arg} {pid}` exited with status {status}");
    }
    Ok(())
}

#[cfg(not(unix))]
fn send_process_signal(_pid: u32, signal: ProcessSignal) -> Result<()> {
    match signal {
        ProcessSignal::Pause => bail!("listen pause is only supported on Unix hosts"),
        ProcessSignal::Resume => bail!("listen resume is only supported on Unix hosts"),
    }
}

fn now_epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};

    use anyhow::{Context, Result};
    use tempfile::tempdir;

    use crate::config::{
        AppConfig, PlanningListenSettings, PlanningMeta, data_root_from_config_path,
    };
    use crate::listen::{
        CanonicalSessionData, LatestResumeHandle, ListenSessionDetail, PullRequestSummary,
        ResumeProvider, SessionOrigin, StaleWorkerFailure, TokenUsage,
    };

    use super::{
        ActiveListenerLock, AgentSession, COMPLETED_SESSION_TTL_SECONDS,
        LISTEN_SESSION_DETAIL_VERSION, ListenProjectPaths, ListenProjectStore, ListenState,
        SessionDetailReferences, SessionPhase, SessionSelector, WorkflowRootLayout,
        now_epoch_seconds, project_key_for_metastack_root, read_json, resolve_source_root,
        sibling_recovery_path, write_json,
    };

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RepairedCanonicalSnapshot {
        provider: Option<String>,
        model: Option<String>,
        reasoning: Option<String>,
        tokens: TokenUsage,
        repair_status: Option<super::CanonicalRepairStatus>,
    }

    fn listen_fixture_path(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("listen")
            .join(name)
    }

    fn read_listen_fixture(name: &str) -> Result<String> {
        let path = listen_fixture_path(name);
        fs::read_to_string(&path).with_context(|| format!("failed to read `{}`", path.display()))
    }

    fn seed_worker_log_fixture(
        store: &ListenProjectStore,
        issue_identifier: &str,
        fixture_name: &str,
    ) -> Result<()> {
        let mut session = default_session(issue_identifier, SessionPhase::Running, 100);
        session.tokens = TokenUsage::default();
        session.latest_resume_handle = None;
        session.log_path = Some(store.log_path(issue_identifier).display().to_string());
        fs::create_dir_all(store.paths().logs_dir.clone())?;
        fs::write(
            store.log_path(issue_identifier),
            read_listen_fixture(fixture_name)?,
        )
        .with_context(|| {
            format!(
                "failed to seed `{fixture_name}` into `{}`",
                store.log_path(issue_identifier).display()
            )
        })?;
        seed_state(store, vec![session])
    }

    fn load_repaired_snapshot(
        fixture_name: &str,
        issue_identifier: &str,
    ) -> Result<(RepairedCanonicalSnapshot, ListenSessionDetail)> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        seed_worker_log_fixture(&store, issue_identifier, fixture_name)?;

        let state = store.load_state()?;
        let repaired = state
            .sessions
            .iter()
            .find(|session| session.issue_identifier == issue_identifier)
            .context("expected repaired session to be present")?;
        let detail = store
            .load_session_detail(issue_identifier)?
            .context("expected repaired detail artifact")?;

        Ok((
            RepairedCanonicalSnapshot {
                provider: repaired.canonical.provider.clone(),
                model: repaired.canonical.model.clone(),
                reasoning: repaired.canonical.reasoning.clone(),
                tokens: repaired.canonical.tokens.clone(),
                repair_status: repaired
                    .canonical
                    .repair
                    .as_ref()
                    .map(|repair| repair.status),
            },
            detail,
        ))
    }

    #[test]
    fn project_store_uses_git_common_dir_source_root_for_worktrees() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let worktree_root = temp.path().join("repo-worktree");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        std::process::Command::new("git")
            .args(["init", "-b", "main", repo_root.to_string_lossy().as_ref()])
            .status()?;
        std::process::Command::new("git")
            .args([
                "-C",
                repo_root.to_string_lossy().as_ref(),
                "config",
                "user.email",
                "test@example.com",
            ])
            .status()?;
        std::process::Command::new("git")
            .args([
                "-C",
                repo_root.to_string_lossy().as_ref(),
                "config",
                "user.name",
                "Meta Test",
            ])
            .status()?;
        fs::write(repo_root.join("README.md"), "repo\n")?;
        std::process::Command::new("git")
            .args(["-C", repo_root.to_string_lossy().as_ref(), "add", "."])
            .status()?;
        std::process::Command::new("git")
            .args([
                "-C",
                repo_root.to_string_lossy().as_ref(),
                "commit",
                "-m",
                "init",
            ])
            .status()?;
        std::process::Command::new("git")
            .args([
                "-C",
                repo_root.to_string_lossy().as_ref(),
                "worktree",
                "add",
                "-b",
                "feature/test",
                worktree_root.to_string_lossy().as_ref(),
                "main",
            ])
            .status()?;

        let source_root = resolve_source_root(&worktree_root)?;
        assert_eq!(source_root.canonicalize()?, repo_root.canonicalize()?);

        Ok(())
    }

    #[test]
    fn project_key_hash_is_stable_for_same_metastack_root() -> Result<()> {
        let temp = tempdir()?;
        let metastack_root = temp.path().join("repo").join(crate::branding::PROJECT_DIR);
        fs::create_dir_all(&metastack_root)?;

        assert_eq!(
            project_key_for_metastack_root(&metastack_root, Some("MetaStack CLI")),
            project_key_for_metastack_root(&metastack_root, Some("metastack cli"))
        );
        assert_ne!(
            project_key_for_metastack_root(&metastack_root, Some("MetaStack CLI")),
            project_key_for_metastack_root(&metastack_root, Some("MetaStack API"))
        );
        assert_ne!(
            project_key_for_metastack_root(&metastack_root, Some("MetaStack CLI")),
            project_key_for_metastack_root(&metastack_root, None)
        );

        Ok(())
    }

    #[test]
    fn project_store_paths_use_global_data_root() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let config_path = temp.path().join("metastack.toml");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let data_root = data_root_from_config_path(&config_path)?;
        let store = ListenProjectStore::resolve_with_data_root(
            &repo_root,
            data_root.clone(),
            Some("MetaStack CLI"),
        )?;
        assert!(data_root.starts_with(temp.path()));
        assert!(store.paths().state_path.starts_with(data_root));
        Ok(())
    }

    fn default_session(
        issue_identifier: &str,
        phase: SessionPhase,
        updated_at: u64,
    ) -> AgentSession {
        AgentSession {
            issue_id: Some(format!("{issue_identifier}-id")),
            issue_identifier: issue_identifier.to_string(),
            issue_title: format!("{issue_identifier} title"),
            project_name: Some("MetaStack CLI".to_string()),
            team_key: "MET".to_string(),
            issue_url: format!("https://linear.app/metastack/{issue_identifier}"),
            phase,
            summary: format!("{issue_identifier} summary"),
            blocked: None,
            brief_path: Some(format!(
                "{}/agents/briefs/{issue_identifier}.md",
                crate::branding::PROJECT_DIR
            )),
            backlog_issue_identifier: Some(format!("TECH-{issue_identifier}")),
            backlog_issue_title: Some(format!("Backlog for {issue_identifier}")),
            backlog_path: Some(format!(
                "{}/backlog/{issue_identifier}",
                crate::branding::PROJECT_DIR
            )),
            workspace_path: Some(format!("/tmp/{issue_identifier}")),
            branch: Some(format!("branch-{issue_identifier}")),
            pull_request: Default::default(),
            workpad_comment_id: Some(format!("workpad-{issue_identifier}")),
            started_at_epoch_seconds: updated_at,
            updated_at_epoch_seconds: updated_at,
            pid: None,
            session_id: Some(format!("session-{issue_identifier}")),
            turn_history: Vec::new(),
            latest_resume_handle: None,
            context_budget_tokens: None,
            pending_linear_sync: None,
            stale_worker_recovery_attempt_count: 0,
            latest_stale_worker_failure: None,
            last_timeout: None,
            turns: Some(1),
            tokens: TokenUsage::default(),
            canonical: CanonicalSessionData::default(),
            log_path: Some(format!("logs/{issue_identifier}.log")),
            origin: SessionOrigin::Listen,
        }
    }

    fn seed_state(store: &ListenProjectStore, sessions: Vec<AgentSession>) -> Result<()> {
        store.ensure_layout()?;
        write_json(
            &store.paths().state_path,
            &ListenState::from_sessions(sessions),
        )
    }

    fn spawn_sleep_process() -> Result<Child> {
        Command::new("sleep")
            .arg("30")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("failed to spawn sleep process for listen store test")
    }

    #[test]
    fn load_state_prefers_backup_recovery_over_temp_candidate() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        store.ensure_layout()?;

        let backup_path = sibling_recovery_path(&store.paths().state_path, "bak");
        let temp_path = sibling_recovery_path(&store.paths().state_path, "tmp");
        let backup_state = ListenState::from_sessions(vec![default_session(
            "ENG-BACKUP",
            SessionPhase::Blocked,
            1,
        )]);
        let temp_state =
            ListenState::from_sessions(vec![default_session("ENG-TMP", SessionPhase::Blocked, 2)]);
        write_json(&backup_path, &backup_state)?;
        write_json(&temp_path, &temp_state)?;
        fs::write(&store.paths().state_path, "{ not valid json")?;

        let recovered = store.load_state()?;
        let rewritten: ListenState = read_json(&store.paths().state_path)?;

        assert_eq!(recovered.sessions.len(), 1);
        assert_eq!(recovered.sessions[0].issue_identifier, "ENG-BACKUP");
        assert_eq!(rewritten.sessions[0].issue_identifier, "ENG-BACKUP");
        assert!(!backup_path.exists());
        assert!(temp_path.exists());
        Ok(())
    }

    #[test]
    fn load_state_removes_consumed_temp_recovery_artifact() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        store.ensure_layout()?;

        let backup_path = sibling_recovery_path(&store.paths().state_path, "bak");
        let temp_path = sibling_recovery_path(&store.paths().state_path, "tmp");
        let temp_state =
            ListenState::from_sessions(vec![default_session("ENG-TMP", SessionPhase::Blocked, 2)]);
        fs::write(&backup_path, "{ invalid backup")?;
        write_json(&temp_path, &temp_state)?;
        fs::write(&store.paths().state_path, "{ not valid json")?;

        let recovered = store.load_state()?;
        let rewritten: ListenState = read_json(&store.paths().state_path)?;

        assert_eq!(recovered.sessions.len(), 1);
        assert_eq!(recovered.sessions[0].issue_identifier, "ENG-TMP");
        assert_eq!(rewritten.sessions[0].issue_identifier, "ENG-TMP");
        assert!(backup_path.exists());
        assert!(!temp_path.exists());
        Ok(())
    }

    #[test]
    fn load_state_reports_attempted_recovery_paths_when_unrecoverable() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        store.ensure_layout()?;

        let backup_path = sibling_recovery_path(&store.paths().state_path, "bak");
        let temp_path = sibling_recovery_path(&store.paths().state_path, "tmp");
        fs::write(&store.paths().state_path, "{ invalid state")?;
        fs::write(&backup_path, "{ invalid backup")?;

        let error = store
            .load_state()
            .expect_err("corrupt primary state without a valid recovery artifact should fail");
        let message = format!("{error:#}");

        assert!(message.contains(store.paths().state_path.to_string_lossy().as_ref()));
        assert!(message.contains(backup_path.to_string_lossy().as_ref()));
        assert!(message.contains(temp_path.to_string_lossy().as_ref()));
        Ok(())
    }

    #[test]
    fn list_projects_recovers_corrupt_project_metadata_from_backup() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store =
            ListenProjectStore::resolve_with_data_root(&repo_root, data_root.clone(), None)?;
        store.ensure_layout()?;

        let backup_path = sibling_recovery_path(&store.paths().project_metadata_path, "bak");
        fs::copy(&store.paths().project_metadata_path, &backup_path)?;
        fs::write(&store.paths().project_metadata_path, "{ invalid metadata")?;

        let projects = ListenProjectStore::list_projects_with_data_root(data_root)?;
        let rewritten: super::ListenProjectMetadata =
            read_json(&store.paths().project_metadata_path)?;

        assert!(
            projects
                .iter()
                .any(|project| project.metadata.project_key == store.identity().project_key)
        );
        assert_eq!(rewritten.project_key, store.identity().project_key);
        assert!(!backup_path.exists());
        Ok(())
    }

    #[test]
    fn acquire_listener_lock_removes_corrupt_orphaned_lock() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        store.ensure_layout()?;
        fs::write(&store.paths().lock_path, "{ invalid lock")?;

        {
            let _guard = store.acquire_listener_lock(std::process::id())?;
            let persisted: ActiveListenerLock = read_json(&store.paths().lock_path)?;
            assert_eq!(persisted.pid, std::process::id());
        }

        assert!(!store.paths().lock_path.exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn acquire_listener_lock_recovers_live_lock_from_backup_before_blocking() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        store.ensure_layout()?;
        let mut child = spawn_sleep_process()?;
        let live_lock = ActiveListenerLock {
            pid: child.id(),
            acquired_at_epoch_seconds: now_epoch_seconds(),
            source_root: repo_root.display().to_string(),
            metastack_root: repo_root
                .join(crate::branding::PROJECT_DIR)
                .display()
                .to_string(),
        };
        let backup_path = sibling_recovery_path(&store.paths().lock_path, "bak");
        write_json(&backup_path, &live_lock)?;
        fs::write(&store.paths().lock_path, "{ invalid lock")?;

        let error = store
            .acquire_listener_lock(std::process::id())
            .expect_err("recovered live lock should still block duplicate listeners");
        let recovered: ActiveListenerLock = read_json(&store.paths().lock_path)?;

        let _ = child.kill();
        let _ = child.wait();

        assert!(error.to_string().contains("already owns project"));
        assert_eq!(recovered.pid, live_lock.pid);
        assert!(!backup_path.exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn acquire_listener_lock_blocks_on_live_backup_when_rewrite_fails() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let resolved_store =
            ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        resolved_store.ensure_layout()?;
        let lock_root = temp.path().join("lock-root");
        fs::create_dir_all(&lock_root)?;
        let lock_layout =
            WorkflowRootLayout::install_scoped(lock_root.clone(), "active-listener.lock.json");
        let store = ListenProjectStore {
            identity: resolved_store.identity.clone(),
            paths: ListenProjectPaths {
                layout: lock_layout.clone(),
                projects_root: resolved_store.paths.projects_root.clone(),
                project_dir: resolved_store.paths.project_dir.clone(),
                project_metadata_path: resolved_store.paths.project_metadata_path.clone(),
                state_path: resolved_store.paths.state_path.clone(),
                lock_path: lock_layout.active_session_path().to_path_buf(),
                logs_dir: resolved_store.paths.logs_dir.clone(),
                details_dir: resolved_store.paths.details_dir.clone(),
                verification_dir: resolved_store.paths.verification_dir.clone(),
            },
        };
        let mut child = spawn_sleep_process()?;
        let live_lock = ActiveListenerLock {
            pid: child.id(),
            acquired_at_epoch_seconds: now_epoch_seconds(),
            source_root: repo_root.display().to_string(),
            metastack_root: repo_root
                .join(crate::branding::PROJECT_DIR)
                .display()
                .to_string(),
        };
        write_json(
            &sibling_recovery_path(&store.paths().lock_path, "bak"),
            &live_lock,
        )?;
        fs::write(&store.paths().lock_path, "{ invalid lock")?;

        let original_mode = fs::metadata(&lock_root)?.permissions().mode();
        let mut read_only = fs::metadata(&lock_root)?.permissions();
        read_only.set_mode(0o555);
        fs::set_permissions(&lock_root, read_only)?;

        let error = store
            .acquire_listener_lock(std::process::id())
            .expect_err("live backup should still block when rewriting the recovered lock fails");

        let mut restored = fs::metadata(&lock_root)?.permissions();
        restored.set_mode(original_mode);
        fs::set_permissions(&lock_root, restored)?;
        let _ = child.kill();
        let _ = child.wait();

        assert!(format!("{error:#}").contains("already owns project"));
        assert_eq!(
            fs::read_to_string(&store.paths().lock_path)?,
            "{ invalid lock"
        );
        Ok(())
    }

    #[test]
    fn listener_lock_guard_does_not_delete_replaced_lock_file() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        store.ensure_layout()?;

        let guard = store.acquire_listener_lock(std::process::id())?;
        let replacement = ActiveListenerLock {
            pid: 999_999,
            acquired_at_epoch_seconds: now_epoch_seconds(),
            source_root: repo_root.display().to_string(),
            metastack_root: repo_root
                .join(crate::branding::PROJECT_DIR)
                .display()
                .to_string(),
        };
        write_json(&store.paths().lock_path, &replacement)?;
        drop(guard);

        let persisted: ActiveListenerLock = read_json(&store.paths().lock_path)?;
        assert_eq!(persisted.pid, replacement.pid);
        Ok(())
    }

    #[test]
    fn clear_by_issue_identifier_preserves_other_sessions_and_project_files() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        store.ensure_layout()?;
        fs::write(store.paths().state_path.clone(), "{}")?;
        write_json(
            &store.paths().lock_path,
            &ActiveListenerLock {
                pid: 99_999,
                acquired_at_epoch_seconds: 0,
                source_root: repo_root.display().to_string(),
                metastack_root: repo_root
                    .join(crate::branding::PROJECT_DIR)
                    .display()
                    .to_string(),
            },
        )?;
        seed_state(
            &store,
            vec![
                default_session("ENG-10163", SessionPhase::Blocked, 100),
                default_session("ENG-10164", SessionPhase::Blocked, 200),
            ],
        )?;

        let outcome =
            store.clear_sessions(&SessionSelector::IssueIdentifier("ENG-10163".to_string()))?;
        let state = store.load_state()?;

        assert_eq!(outcome.cleared_sessions.len(), 1);
        assert_eq!(outcome.cleared_sessions[0].issue_identifier, "ENG-10163");
        assert_eq!(outcome.remaining_sessions, 1);
        assert!(store.paths().project_dir.is_dir());
        assert!(store.paths().project_metadata_path.is_file());
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].issue_identifier, "ENG-10164");
        assert!(
            store
                .paths()
                .lock_path
                .parent()
                .is_some_and(|parent| parent.is_dir())
        );
        Ok(())
    }

    #[test]
    fn clear_stale_removes_only_dead_pid_sessions() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;

        let mut stale = default_session("ENG-10163", SessionPhase::Blocked, 100);
        stale.pid = Some(99_999);
        let mut running = default_session("ENG-10164", SessionPhase::Running, 200);
        let mut child = spawn_sleep_process()?;
        running.pid = Some(child.id());
        seed_state(&store, vec![stale, running.clone()])?;

        let outcome = store.clear_sessions(&SessionSelector::Stale)?;
        let state = store.load_state()?;

        let _ = child.kill();
        let _ = child.wait();

        assert_eq!(outcome.cleared_sessions.len(), 1);
        assert_eq!(outcome.cleared_sessions[0].issue_identifier, "ENG-10163");
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].issue_identifier, "ENG-10164");
        Ok(())
    }

    #[test]
    fn clear_refuses_live_targeted_sessions() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;

        let mut live = default_session("ENG-10163", SessionPhase::Running, 100);
        let mut child = spawn_sleep_process()?;
        live.pid = Some(child.id());
        seed_state(&store, vec![live])?;

        let error = store
            .clear_sessions(&SessionSelector::All)
            .expect_err("live session clear should fail");

        let _ = child.kill();
        let _ = child.wait();

        assert!(
            error
                .to_string()
                .contains("cannot clear live MetaListen session record(s)")
        );
        assert_eq!(store.load_state()?.sessions.len(), 1);
        Ok(())
    }

    #[test]
    fn load_state_prunes_only_completed_sessions_older_than_ttl() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        let now = super::now_epoch_seconds();
        let ttl = COMPLETED_SESSION_TTL_SECONDS;

        seed_state(
            &store,
            vec![
                default_session("ENG-10163", SessionPhase::Completed, now - ttl - 1),
                default_session("ENG-10164", SessionPhase::Completed, now - ttl + 5),
                default_session("ENG-10165", SessionPhase::Blocked, now - ttl - 1),
            ],
        )?;

        let state = store.load_state()?;

        assert_eq!(state.sessions.len(), 2);
        assert!(
            state
                .sessions
                .iter()
                .any(|session| session.issue_identifier == "ENG-10164")
        );
        assert!(
            state
                .sessions
                .iter()
                .any(|session| session.issue_identifier == "ENG-10165")
        );
        assert!(
            !state
                .sessions
                .iter()
                .any(|session| session.issue_identifier == "ENG-10163")
        );
        Ok(())
    }

    #[test]
    fn remove_ticket_artifacts_cleans_detail_and_log_files() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        store.ensure_layout()?;

        let issue_identifier = "ENG-10163";
        let mut session = default_session(issue_identifier, SessionPhase::Running, 100);
        session.log_path = Some(store.log_path(issue_identifier).display().to_string());
        fs::write(store.log_path(issue_identifier), "worker log line\n")
            .context("failed to seed session log for listen store test")?;
        store.save_state(&ListenState::from_sessions(vec![session]))?;

        assert!(store.detail_path(issue_identifier).is_file());
        assert!(store.log_path(issue_identifier).is_file());

        store.remove_ticket_artifacts(issue_identifier)?;

        assert!(store.load_state()?.sessions.is_empty());
        assert!(!store.detail_path(issue_identifier).exists());
        assert!(!store.log_path(issue_identifier).exists());
        Ok(())
    }

    #[test]
    fn remove_ticket_artifacts_cleans_orphaned_detail_without_session_state() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        store.ensure_layout()?;

        let issue_identifier = "ENG-10163";
        fs::write(
            store.detail_path(issue_identifier),
            serde_json::to_vec_pretty(&ListenSessionDetail {
                version: LISTEN_SESSION_DETAIL_VERSION,
                issue_identifier: issue_identifier.to_string(),
                issue_title: "orphan detail".to_string(),
                started_at_epoch_seconds: 100,
                updated_at_epoch_seconds: 100,
                session_updated_at_epoch_seconds: 100,
                phase: SessionPhase::Completed,
                summary: "detail without state".to_string(),
                blocked: None,
                turns: Some(1),
                tokens: TokenUsage::default(),
                turn_history: Vec::new(),
                context_budget_tokens: None,
                canonical: CanonicalSessionData::default(),
                pull_request: PullRequestSummary::default(),
                verification: None,
                latest_resume_handle: None,
                pending_linear_sync: None,
                stale_worker_recovery_attempt_count: 0,
                latest_stale_worker_failure: None,
                last_timeout: None,
                references: SessionDetailReferences::default(),
                prompt_context: Vec::new(),
                milestones: Vec::new(),
                log_excerpts: Vec::new(),
            })?,
        )
        .context("failed to seed orphaned detail artifact for listen store test")?;
        fs::write(store.log_path(issue_identifier), "worker log line\n")
            .context("failed to seed orphaned log file for listen store test")?;

        assert!(store.load_state()?.sessions.is_empty());
        assert!(store.detail_path(issue_identifier).is_file());
        assert!(store.log_path(issue_identifier).is_file());

        store.remove_ticket_artifacts(issue_identifier)?;

        assert!(store.load_state()?.sessions.is_empty());
        assert!(!store.detail_path(issue_identifier).exists());
        assert!(!store.log_path(issue_identifier).exists());
        Ok(())
    }

    #[test]
    fn invalid_session_detail_is_treated_as_unavailable_and_rewritten() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        store.ensure_layout()?;

        let issue_identifier = "ENG-10163";
        fs::write(store.detail_path(issue_identifier), "{ not valid json")
            .context("failed to seed invalid detail artifact for listen store test")?;

        assert!(store.load_session_detail(issue_identifier)?.is_none());

        let mut session = default_session(issue_identifier, SessionPhase::Running, 100);
        session.log_path = Some(store.log_path(issue_identifier).display().to_string());
        session.latest_resume_handle = Some(LatestResumeHandle {
            provider: ResumeProvider::Codex,
            id: "thread-ENG-10163".to_string(),
        });
        store.save_state(&ListenState::from_sessions(vec![session]))?;

        let detail = store
            .load_session_detail(issue_identifier)?
            .context("expected save_state to rewrite the invalid detail artifact")?;
        assert_eq!(detail.issue_identifier, issue_identifier);
        assert_eq!(detail.summary, format!("{issue_identifier} summary"));
        assert_eq!(detail.version, LISTEN_SESSION_DETAIL_VERSION);
        assert_eq!(
            detail.latest_resume_handle,
            Some(LatestResumeHandle {
                provider: ResumeProvider::Codex,
                id: "thread-ENG-10163".to_string(),
            })
        );
        Ok(())
    }

    #[test]
    fn load_session_details_resolves_repo_context_budget_for_workspace() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let workspace_root = temp.path().join("workspace");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        fs::create_dir_all(workspace_root.join(crate::branding::PROJECT_DIR))?;
        PlanningMeta {
            listen: PlanningListenSettings {
                context_budget_tokens: Some(100_000),
                ..PlanningListenSettings::default()
            },
            ..PlanningMeta::default()
        }
        .save(&workspace_root)?;

        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        let mut session = default_session("ENG-10782", SessionPhase::Running, 100);
        session.workspace_path = Some(workspace_root.display().to_string());
        store.save_state(&ListenState::from_sessions(vec![session.clone()]))?;

        let details = store.load_session_details(&AppConfig::default(), &[session])?;

        assert_eq!(details.len(), 1);
        assert_eq!(details[0].context_budget_tokens, Some(100_000));
        Ok(())
    }

    #[test]
    fn load_session_details_prefers_persisted_session_context_budget() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let workspace_root = temp.path().join("workspace");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        fs::create_dir_all(workspace_root.join(crate::branding::PROJECT_DIR))?;
        PlanningMeta {
            listen: PlanningListenSettings {
                context_budget_tokens: Some(100_000),
                ..PlanningListenSettings::default()
            },
            ..PlanningMeta::default()
        }
        .save(&workspace_root)?;

        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        let mut session = default_session("ENG-10782", SessionPhase::Running, 100);
        session.workspace_path = Some(workspace_root.display().to_string());
        session.context_budget_tokens = Some(90_000);
        store.save_state(&ListenState::from_sessions(vec![session.clone()]))?;

        let details = store.load_session_details(&AppConfig::default(), &[session])?;

        assert_eq!(details.len(), 1);
        assert_eq!(details[0].context_budget_tokens, Some(90_000));
        Ok(())
    }

    #[test]
    fn list_projects_uses_pruned_state_for_latest_session() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        let now = super::now_epoch_seconds();

        seed_state(
            &store,
            vec![
                default_session(
                    "ENG-10163",
                    SessionPhase::Completed,
                    now - COMPLETED_SESSION_TTL_SECONDS - 1,
                ),
                default_session("ENG-10164", SessionPhase::Blocked, now),
            ],
        )?;

        let projects = ListenProjectStore::list_projects_with_data_root(temp.path().join("data"))?;
        let summary = projects
            .into_iter()
            .find(|project| project.metadata.project_key == store.identity().project_key)
            .expect("project summary should exist");

        assert_eq!(
            summary
                .latest_session
                .as_ref()
                .map(|session| session.issue_identifier.as_str()),
            Some("ENG-10164")
        );
        Ok(())
    }

    #[test]
    fn retry_blocked_session_resets_to_brief_ready() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        let now = super::now_epoch_seconds();

        seed_state(
            &store,
            vec![
                default_session("ENG-100", SessionPhase::Blocked, now),
                default_session("ENG-200", SessionPhase::Running, now),
            ],
        )?;

        assert!(store.retry_blocked_session("ENG-100")?);

        let state = store.load_state()?;
        let retried = state
            .sessions
            .iter()
            .find(|s| s.issue_identifier == "ENG-100")
            .expect("session should exist");
        assert_eq!(retried.phase, SessionPhase::BriefReady);
        assert!(retried.pid.is_none());
        assert_eq!(retried.summary, "Retrying from previous workspace state");

        let other = state
            .sessions
            .iter()
            .find(|s| s.issue_identifier == "ENG-200")
            .expect("other session should be untouched");
        assert_eq!(other.phase, SessionPhase::Running);

        Ok(())
    }

    #[test]
    fn retry_blocked_session_resets_stale_worker_recovery_window() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        let now = super::now_epoch_seconds();

        let mut session = default_session("ENG-777", SessionPhase::Blocked, now);
        session.started_at_epoch_seconds = now.saturating_sub(600);
        session.stale_worker_recovery_attempt_count = 2;
        session.latest_stale_worker_failure = Some(StaleWorkerFailure {
            pid: 9_001,
            observed_at_epoch_seconds: now.saturating_sub(60),
            last_persisted_phase: SessionPhase::Running,
            summary: "worker pid 9001 disappeared while the session was running".to_string(),
            classification: super::super::blocked_reason(
                super::super::BlockedCategory::Infra,
                "stale worker retry budget exhausted",
                false,
            ),
        });
        seed_state(&store, vec![session])?;

        assert!(store.retry_blocked_session("ENG-777")?);

        let state = store.load_state()?;
        let retried = state
            .sessions
            .iter()
            .find(|session| session.issue_identifier == "ENG-777")
            .expect("session should exist");
        assert_eq!(retried.phase, SessionPhase::BriefReady);
        assert_eq!(retried.stale_worker_recovery_attempt_count, 0);
        assert!(retried.latest_stale_worker_failure.is_none());
        assert!(retried.started_at_epoch_seconds >= now);

        Ok(())
    }

    #[test]
    fn load_state_backfills_stale_worker_fields_from_old_payloads() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        store.ensure_layout()?;

        let issue_identifier = "ENG-778";
        fs::write(
            &store.paths().state_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "version": 1,
                "sessions": [{
                    "issue_id": format!("{issue_identifier}-id"),
                    "issue_identifier": issue_identifier,
                    "issue_title": "Legacy stale worker state",
                    "project_name": "MetaStack CLI",
                    "team_key": "ENG",
                    "issue_url": format!("https://linear.app/metastack/{issue_identifier}"),
                    "phase": "blocked",
                    "summary": "Blocked | worker died",
                    "updated_at_epoch_seconds": 1_773_575_100u64
                }]
            }))?,
        )?;
        fs::write(
            store.detail_path(issue_identifier),
            serde_json::to_vec_pretty(&serde_json::json!({
                "version": 4,
                "issue_identifier": issue_identifier,
                "issue_title": "Legacy stale worker state",
                "updated_at_epoch_seconds": 1_773_575_180u64,
                "session_updated_at_epoch_seconds": 1_773_575_100u64,
                "phase": "blocked",
                "summary": "Blocked | worker died",
                "stale_worker_recovery_attempt_count": 1,
                "latest_stale_worker_failure": {
                    "pid": 42424,
                    "observed_at_epoch_seconds": 1_773_575_150u64,
                    "last_persisted_phase": "running",
                    "summary": "worker pid 42424 disappeared while the session was running",
                    "classification": {
                        "category": "infra",
                        "reason": "worker died",
                        "retryable": true
                    }
                },
                "references": {},
                "milestones": [],
                "log_excerpts": []
            }))?,
        )?;

        let state = store.load_state()?;
        let session = state
            .sessions
            .iter()
            .find(|session| session.issue_identifier == issue_identifier)
            .expect("session should exist");
        assert_eq!(session.started_at_epoch_seconds, 1_773_575_100);
        assert_eq!(session.stale_worker_recovery_attempt_count, 1);
        assert_eq!(
            session
                .latest_stale_worker_failure
                .as_ref()
                .map(|failure| failure.pid),
            Some(42_424)
        );

        let detail = store
            .load_session_detail(issue_identifier)?
            .expect("detail should exist");
        assert_eq!(detail.version, LISTEN_SESSION_DETAIL_VERSION);
        assert_eq!(detail.started_at_epoch_seconds, 1_773_575_100);
        assert_eq!(detail.stale_worker_recovery_attempt_count, 1);
        assert_eq!(
            detail
                .latest_stale_worker_failure
                .as_ref()
                .map(|failure| failure.pid),
            Some(42_424)
        );

        Ok(())
    }

    #[test]
    fn retry_blocked_session_returns_false_for_non_blocked() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        let now = super::now_epoch_seconds();

        seed_state(
            &store,
            vec![default_session("ENG-300", SessionPhase::Running, now)],
        )?;

        assert!(!store.retry_blocked_session("ENG-300")?);
        assert!(!store.retry_blocked_session("ENG-999")?);

        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn pause_running_session_marks_session_paused_and_keeps_pid() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        let now = super::now_epoch_seconds();
        let mut child = spawn_sleep_process()?;
        let pid = child.id();

        let mut session = default_session("ENG-400", SessionPhase::Running, now);
        session.pid = Some(pid);
        seed_state(&store, vec![session])?;

        assert!(store.pause_running_session("ENG-400")?);

        let state = store.load_state()?;
        let paused = state
            .sessions
            .iter()
            .find(|s| s.issue_identifier == "ENG-400")
            .expect("session should exist");
        assert_eq!(paused.phase, SessionPhase::Paused);
        assert_eq!(paused.pid, Some(pid));
        assert!(paused.summary.contains("Paused by operator"));
        assert!(super::pid_is_running(pid));

        let _ = child.kill();
        let _ = child.wait();
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn resume_paused_session_marks_session_running_without_changing_pid() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        let now = super::now_epoch_seconds();
        let mut child = spawn_sleep_process()?;
        let pid = child.id();

        let mut session = default_session("ENG-401", SessionPhase::Running, now);
        session.pid = Some(pid);
        seed_state(&store, vec![session])?;
        assert!(store.pause_running_session("ENG-401")?);

        assert!(store.resume_paused_session("ENG-401")?);

        let state = store.load_state()?;
        let resumed = state
            .sessions
            .iter()
            .find(|s| s.issue_identifier == "ENG-401")
            .expect("session should exist");
        assert_eq!(resumed.phase, SessionPhase::Running);
        assert_eq!(resumed.pid, Some(pid));
        assert!(resumed.summary.contains("Resumed by operator"));
        assert!(super::pid_is_running(pid));

        let _ = child.kill();
        let _ = child.wait();
        Ok(())
    }

    #[test]
    fn pause_and_resume_return_false_when_session_cannot_transition() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;
        let now = super::now_epoch_seconds();

        seed_state(
            &store,
            vec![
                default_session("ENG-500", SessionPhase::Blocked, now),
                default_session("ENG-501", SessionPhase::Paused, now),
                default_session("ENG-502", SessionPhase::Running, now),
            ],
        )?;

        assert!(!store.pause_running_session("ENG-500")?);
        assert!(!store.pause_running_session("ENG-502")?);
        assert!(!store.resume_paused_session("ENG-500")?);
        assert!(!store.resume_paused_session("ENG-501")?);
        assert!(!store.pause_running_session("ENG-999")?);
        assert!(!store.resume_paused_session("ENG-999")?);

        Ok(())
    }

    #[test]
    fn load_state_repairs_equivalent_canonical_metadata_from_branded_and_legacy_turn_fixtures()
    -> Result<()> {
        let (branded, branded_detail) =
            load_repaired_snapshot("turn-branded-intu.log", "ENG-10170")?;
        let (legacy, legacy_detail) = load_repaired_snapshot("turn-legacy-meta.log", "ENG-10171")?;

        let expected = RepairedCanonicalSnapshot {
            provider: Some("claude".to_string()),
            model: Some("sonnet".to_string()),
            reasoning: Some("high".to_string()),
            tokens: TokenUsage {
                input: Some(210),
                output: Some(34),
            },
            repair_status: Some(super::CanonicalRepairStatus::Recovered),
        };
        assert_eq!(branded, expected);
        assert_eq!(legacy, expected);
        assert_eq!(branded_detail.canonical.provider.as_deref(), Some("claude"));
        assert_eq!(legacy_detail.canonical.provider.as_deref(), Some("claude"));
        assert_eq!(branded_detail.canonical.tokens.input, Some(210));
        assert_eq!(legacy_detail.canonical.tokens.output, Some(34));

        Ok(())
    }

    #[test]
    fn save_state_persists_claude_canonical_tokens_in_detail_artifact() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;

        let issue_identifier = "ENG-10172";
        let mut session = default_session(issue_identifier, SessionPhase::Completed, 100);
        session.tokens = TokenUsage {
            input: Some(210),
            output: Some(34),
        };
        session.canonical = CanonicalSessionData {
            provider: Some("claude".to_string()),
            model: Some("sonnet".to_string()),
            reasoning: Some("high".to_string()),
            tokens: session.tokens.clone(),
            repair: None,
        };

        store.save_state(&ListenState::from_sessions(vec![session]))?;

        let detail = store
            .load_session_detail(issue_identifier)?
            .context("expected persisted detail artifact")?;
        assert_eq!(detail.tokens.input, Some(210));
        assert_eq!(detail.tokens.output, Some(34));
        assert_eq!(detail.canonical.provider.as_deref(), Some("claude"));
        assert_eq!(detail.canonical.model.as_deref(), Some("sonnet"));
        assert_eq!(detail.canonical.reasoning.as_deref(), Some("high"));
        assert_eq!(detail.canonical.tokens.input, Some(210));
        assert_eq!(detail.canonical.tokens.output, Some(34));

        Ok(())
    }

    #[test]
    fn load_state_marks_unrecoverable_historical_sessions_as_skipped() -> Result<()> {
        let temp = tempdir()?;
        let repo_root = temp.path().join("repo");
        let data_root = temp.path().join("data");
        fs::create_dir_all(repo_root.join(crate::branding::PROJECT_DIR))?;
        let store = ListenProjectStore::resolve_with_data_root(&repo_root, data_root, None)?;

        let issue_identifier = "ENG-10171";
        let mut session = default_session(issue_identifier, SessionPhase::Blocked, 100);
        session.tokens = TokenUsage::default();
        session.latest_resume_handle = None;
        session.log_path = Some(store.log_path(issue_identifier).display().to_string());
        fs::create_dir_all(store.paths().logs_dir.clone())?;
        fs::write(
            store.log_path(issue_identifier),
            format!(
                "{}1/20 @ 2026-03-23T12:00:00Z ---\n\
                 Resolved provider: codex\n\
                 {}2/20 @ 2026-03-23T12:01:00Z ---\n\
                 Resolved provider: claude\n",
                super::listen_turn_log_prefix(),
                super::listen_turn_log_prefix(),
            ),
        )?;
        seed_state(&store, vec![session])?;

        let state = store.load_state()?;
        let skipped = state
            .sessions
            .iter()
            .find(|session| session.issue_identifier == issue_identifier)
            .context("expected skipped session to be present")?;
        assert!(skipped.canonical.provider.is_none());
        assert!(!skipped.canonical.tokens.is_known());
        assert_eq!(
            skipped
                .canonical
                .repair
                .as_ref()
                .map(|repair| repair.status),
            Some(super::CanonicalRepairStatus::Skipped)
        );
        assert!(
            skipped
                .canonical
                .repair
                .as_ref()
                .and_then(|repair| repair.note.as_deref())
                .is_some_and(|note| note.contains("ambiguous provider evidence"))
        );

        Ok(())
    }

    #[test]
    fn load_state_treats_branded_and_legacy_preflight_only_fixtures_as_skipped_repair() -> Result<()>
    {
        let (branded, _) = load_repaired_snapshot("preflight-only-branded-intu.log", "ENG-10172")?;
        let (legacy, _) = load_repaired_snapshot("preflight-only-legacy-meta.log", "ENG-10173")?;

        let expected = RepairedCanonicalSnapshot {
            provider: None,
            model: None,
            reasoning: None,
            tokens: TokenUsage::default(),
            repair_status: Some(super::CanonicalRepairStatus::Skipped),
        };
        assert_eq!(branded, expected);
        assert_eq!(legacy, expected);

        Ok(())
    }

    #[test]
    fn load_state_keeps_preflight_boundaries_from_corrupting_later_valid_turn_repair() -> Result<()>
    {
        let (branded, _) =
            load_repaired_snapshot("preflight-then-turn-branded-intu.log", "ENG-10174")?;
        let (legacy, _) =
            load_repaired_snapshot("preflight-then-turn-legacy-meta.log", "ENG-10175")?;

        let expected = RepairedCanonicalSnapshot {
            provider: Some("claude".to_string()),
            model: Some("sonnet".to_string()),
            reasoning: Some("high".to_string()),
            tokens: TokenUsage {
                input: Some(210),
                output: Some(34),
            },
            repair_status: Some(super::CanonicalRepairStatus::Recovered),
        };
        assert_eq!(branded, expected);
        assert_eq!(legacy, expected);

        Ok(())
    }

    #[test]
    fn load_state_repairs_mixed_legacy_and_branded_worker_log_fixture() -> Result<()> {
        let (repaired, detail) =
            load_repaired_snapshot("mixed-legacy-and-branded.log", "ENG-10176")?;

        assert_eq!(
            repaired,
            RepairedCanonicalSnapshot {
                provider: Some("claude".to_string()),
                model: Some("sonnet".to_string()),
                reasoning: Some("high".to_string()),
                tokens: TokenUsage {
                    input: Some(290),
                    output: Some(47),
                },
                repair_status: Some(super::CanonicalRepairStatus::Recovered),
            }
        );
        assert_eq!(detail.canonical.provider.as_deref(), Some("claude"));
        assert_eq!(detail.canonical.model.as_deref(), Some("sonnet"));
        assert_eq!(detail.canonical.reasoning.as_deref(), Some("high"));
        assert_eq!(detail.canonical.tokens.input, Some(290));
        assert_eq!(detail.canonical.tokens.output, Some(47));

        Ok(())
    }
}

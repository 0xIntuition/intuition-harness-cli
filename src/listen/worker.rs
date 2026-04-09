use std::cell::RefCell;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::time::sleep;

use crate::agent_provider::builtin_provider_adapter;
use crate::agents::{
    AgentExecutionOptions, AgentTokenUsage, apply_invocation_environment,
    apply_noninteractive_agent_environment, command_args_for_invocation,
    command_args_for_invocation_with_options, format_agent_config_source,
    render_invocation_diagnostics, resolve_agent_invocation_for_planning,
    validate_invocation_command_surface,
};
use crate::backlog::load_issue_metadata;
use crate::cli::{ListenWorkerArgs, RunAgentArgs};
use crate::config::{
    AGENT_ROUTE_AGENTS_LISTEN, AGENT_ROUTE_AGENTS_LISTEN_VERIFICATION, AppConfig, LinearConfig,
    LinearConfigOverrides, PlanningMeta, PromptTransport,
};
use crate::config_resolution::{AgentConfigOverrides, normalize_agent_name, resolve_agent_config};
use crate::fs::sibling_workspace_root;
use crate::fs::{PlanningPaths, canonicalize_existing_dir, write_text_file};
use crate::github_pr::{
    GhCli, PullRequestCheck, PullRequestChecksOutcome, PullRequestLifecycleResult,
    PullRequestPublishMode, PullRequestPublishRequest, ResolvedBranchPullRequest, WorkflowRun,
};
use crate::linear::{
    AttachmentCreateRequest, IssueListFilters, IssueSummary, LinearClient, LinearService,
    ReqwestLinearClient, WorkflowState, classify_linear_failure,
};
use crate::managed_child::{
    ManagedChild, ManagedChildOutput, ManagedChildResult, ManagedChildSettings,
    ManagedChildTermination,
};
use crate::repo_target::RepoTarget;
use crate::validation::{
    ResolvedValidationProfile, ValidationCommandRecord, resolve_validation_profile,
    run_validation_commands,
};
use crate::workflow_contract::render_workflow_contract_for_listen;
use crate::workspace::{AutoCleanOutcome, try_auto_clean_workspace};

use super::state::{ContextPressure, completed_turn_known_input_tokens};
use super::verification::{
    BattleTestInput, VerificationBattleTestCase, VerificationBattleTestReport,
    VerificationCodeReviewReport, VerificationCriterionResult, VerificationE2eReport,
    VerificationE2eStepReport, VerificationFinding, VerificationRecipeStep, VerificationReport,
    VerificationRouteDiagnostics, VerificationStatus, VerificationSummary,
    builtin_quality_criteria, discover_battle_test_inputs, load_route_verification_recipe,
    truncate_for_evidence,
};
use super::workpad::{
    context_checkpoint_present, effective_workpad_body, extract_context_checkpoint,
    extract_unmanaged_review_notes, extract_workpad_section_lines,
};
use super::{
    BACKLOG_STATE, BlockedCategory, BlockedReason, CanonicalSessionData, LatestResumeHandle,
    MAX_STALLED_TURNS, PendingLinearSync, PendingPullRequestAttachment, PullRequestStatus,
    PullRequestSummary, ResumeProvider, SessionPhase, SessionTimeoutRecord,
    SessionTimeoutTermination, TokenUsage, TurnPromptMode, TurnTokenSnapshot, agent_log_path,
    backlog_progress_for_issue_dir, blocked_reason, capture_workspace_snapshot,
    compact_blocked_summary, compact_completed_summary, compact_running_summary,
    compact_session_summary, compare_workspace_snapshots, current_workspace_branch,
    issue_state_label, issue_team_key, listen_issue_is_active, now_epoch_seconds, now_timestamp,
    preflight, render_agent_prompt, render_continuation_prompt,
    try_transition_issue_to_review_state, workspace_has_meaningful_progress, write_listen_session,
};

const REQUIRED_LISTEN_PR_LABEL: &str = "metastack";
const LEGACY_LISTEN_PR_LABEL: &str = "symphony";
const REQUIRED_LISTEN_PR_LABEL_COLOR: &str = "0e8a16";
const REQUIRED_LISTEN_PR_LABEL_DESCRIPTION: &str = "MetaStack automation";
const LINEAR_IDENTIFIER_PR_LABEL_COLOR: &str = "1d76db";
const LISTEN_PULL_REQUEST_BASE_BRANCH: &str = "main";
const REQUIRED_VERIFICATION_WORKFLOW_NAME: &str = "quality";
const EXACT_SHA_QUALITY_CRITERION: &str =
    "The active branch PR has a passing `quality` workflow result for the exact current HEAD SHA.";
const HIGH_RISK_LISTEN_MODULES: [&str; 3] = [
    "src/listen/mod.rs",
    "src/listen/store.rs",
    "src/listen/worker.rs",
];
const CROSS_ACTOR_REVIEW_HEURISTICS: [&str; 4] = [
    "Inspect cross-actor state ownership for blocked metadata, canonical metadata, `turn_history`, and `latest_resume_handle`.",
    "Inspect spawn-before-persist flows after concurrent worker launch.",
    "Inspect whole-state rewrites and stale snapshot overwrites of long-lived listen session state.",
    "Inspect adjacent state-write paths outside the immediate diff in the high-risk listen modules.",
];
fn listen_preflight_failure_header(timestamp: &str) -> String {
    format!(
        "\n--- {} listen preflight failed @ {} ---\n",
        crate::branding::COMMAND_NAME,
        timestamp
    )
}

pub(super) async fn run_listen_worker(args: &ListenWorkerArgs) -> Result<()> {
    let source_root = canonicalize_existing_dir(&args.source_root)?;
    let workspace_path = canonicalize_existing_dir(&args.workspace)?;
    let planning_meta = crate::config::PlanningMeta::load(&source_root)?;
    let project_selector = args
        .project
        .as_deref()
        .or(planning_meta.linear.project_id.as_deref());
    let app_config = AppConfig::load()?;
    let linear_config = LinearConfig::new_with_root(
        Some(&source_root),
        LinearConfigOverrides {
            api_key: args.api_key.clone(),
            api_url: args.api_url.clone(),
            default_team: args.team.clone(),
            profile: args.profile.clone(),
        },
    )?;
    let service = LinearService::new(
        ReqwestLinearClient::new(linear_config.clone())?,
        linear_config.default_team.clone(),
    );
    let log_path = agent_log_path(&source_root, args.project.as_deref(), &args.issue);
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open `{}`", log_path.display()))?;
    let branch = current_workspace_branch(&workspace_path).ok();
    let worker_pid = std::process::id();
    let mut turns_completed =
        load_existing_turn_count(&source_root, project_selector, &args.issue)?;
    let mut pending_linear_sync =
        load_existing_pending_linear_sync(&source_root, project_selector, &args.issue)?;
    let existing_issue_snapshot =
        load_existing_issue_snapshot(&source_root, project_selector, &args.issue)?;
    let initial_meaningful_progress = workspace_has_meaningful_progress(&workspace_path, true)?;
    let mut issue = match load_worker_issue(&service, &args.issue).await {
        Ok(issue) => issue,
        Err(error) => {
            let Some(existing_issue) = existing_issue_snapshot else {
                return Err(error);
            };
            if !initial_meaningful_progress && turns_completed == 0 && pending_linear_sync.is_none()
            {
                return Err(error);
            }
            defer_pending_linear_sync_operation(
                &app_config,
                &mut pending_linear_sync,
                &error,
                "issue refresh",
                &log_path,
                |pending| {
                    pending.require_issue_refresh = true;
                },
            )?;
            existing_issue
        }
    };
    let backlog_issue = match args.backlog_issue.as_deref() {
        Some(identifier) => Some(load_worker_backlog_issue(
            &workspace_path,
            identifier,
            &issue,
        )?),
        None => None,
    };
    let turn_context = ListenTurnContext {
        app_config: &app_config,
        planning_meta: &planning_meta,
        args,
        source_root: &source_root,
        project_selector,
        workspace_path: &workspace_path,
        workpad_comment_id: &args.workpad_comment_id,
        backlog_issue: backlog_issue.as_ref(),
        max_turns: args.max_turns,
        context_budget_tokens: args.context_budget_tokens,
    };
    let session_origin = load_existing_session_origin(&source_root, project_selector, &args.issue)?;
    let mut session_context = WorkerSessionContext {
        source_root: &source_root,
        project_selector,
        workspace_path: &workspace_path,
        branch: branch.as_deref(),
        workpad_comment_id: &args.workpad_comment_id,
        backlog_issue: backlog_issue.as_ref(),
        pid: Some(worker_pid),
        latest_resume_handle: load_existing_latest_resume_handle(
            &source_root,
            project_selector,
            &args.issue,
        )?,
        context_budget_tokens: args.context_budget_tokens,
        pending_linear_sync,
        started_at_epoch_seconds: load_existing_started_at_epoch_seconds(
            &source_root,
            project_selector,
            &args.issue,
        )?,
        stale_worker_recovery_attempt_count: load_existing_stale_worker_recovery_attempt_count(
            &source_root,
            project_selector,
            &args.issue,
        )?,
        latest_stale_worker_failure: load_existing_latest_stale_worker_failure(
            &source_root,
            project_selector,
            &args.issue,
        )?,
        last_timeout: load_existing_last_timeout(&source_root, project_selector, &args.issue)?,
        turn_history: load_existing_turn_history(&source_root, project_selector, &args.issue)?,
        canonical: load_existing_session_canonical(&source_root, project_selector, &args.issue)?,
        pull_request: load_existing_pull_request(&source_root, project_selector, &args.issue)?,
        github_ci_summary: None,
        verification_summary: load_existing_verification_summary(
            &source_root,
            project_selector,
            &args.issue,
        )?,
        origin: session_origin,
    };
    let mut session_tokens =
        load_existing_session_tokens(&source_root, project_selector, &args.issue)?;
    let mut provider_session_id =
        load_existing_provider_session_id(&source_root, project_selector, &args.issue)?;
    let mut stalled_turns = 0u32;
    let mut last_review: Option<ReviewReport> = None;
    let validation_repair_attempts = PlanningMeta::load(&workspace_path)
        .with_context(|| {
            format!(
                "failed to load repo validation settings from `{}`",
                workspace_path.display()
            )
        })?
        .validation
        .repair_attempts();
    let mut remaining_verification_repair_turns = validation_repair_attempts;
    let mut remaining_validation_repair_turns = validation_repair_attempts;
    if let Err(error) = preflight::run_listen_preflight(
        &service,
        &linear_config,
        &app_config,
        &planning_meta,
        preflight::ListenPreflightRequest {
            working_dir: &workspace_path,
            agent: args.agent.as_deref(),
            model: args.model.as_deref(),
            reasoning: args.reasoning.as_deref(),
            require_write_access: true,
        },
    )
    .await
    {
        write_preflight_failure(&log_path, &error)?;
        let blocked = blocked_reason(BlockedCategory::Setup, "missing exec capability", false);
        write_listen_session(
            &source_root,
            project_selector,
            build_worker_blocked_session(
                &issue,
                blocked.clone(),
                compact_blocked_summary(&blocked, issue.description.as_deref(), &log_path),
                &session_context,
                turns_completed,
                provider_session_id.as_deref(),
                &session_context.canonical,
            ),
        )?;
        return Err(error);
    }
    loop {
        match replay_pending_linear_sync(
            &service,
            &mut issue,
            &turn_context,
            &mut session_context,
            turns_completed,
            provider_session_id.as_deref(),
            &log_path,
        )
        .await?
        {
            PendingLinearSyncReplayOutcome::Completed => return Ok(()),
            PendingLinearSyncReplayOutcome::Pending
                if session_context
                    .pending_linear_sync
                    .as_ref()
                    .is_some_and(PendingLinearSync::blocks_agent_turns) =>
            {
                write_pending_linear_sync_blocked_session(
                    &issue,
                    &session_context,
                    turns_completed,
                    provider_session_id.as_deref(),
                    &log_path,
                )?;
                return Ok(());
            }
            PendingLinearSyncReplayOutcome::Pending | PendingLinearSyncReplayOutcome::Cleared => {}
        }

        if !listen_issue_is_active(issue.state.as_ref().map(|state| state.name.as_str())) {
            write_listen_session(
                &source_root,
                project_selector,
                build_worker_session(
                    &issue,
                    SessionPhase::Completed,
                    compact_completed_summary(
                        issue.description.as_deref(),
                        turns_completed,
                        &issue_state_label(&issue),
                    ),
                    &session_context,
                    turns_completed,
                    provider_session_id.as_deref(),
                    &session_context.canonical,
                ),
            )?;
            try_listener_auto_clean(&source_root, project_selector, &workspace_path, &args.issue);
            return Ok(());
        }

        if turns_completed >= args.max_turns {
            let blocked = blocked_reason(BlockedCategory::Turn, "turn limit reached", false);
            write_listen_session(
                &source_root,
                project_selector,
                build_worker_blocked_session(
                    &issue,
                    blocked.clone(),
                    compact_blocked_summary(&blocked, issue.description.as_deref(), &log_path),
                    &session_context,
                    turns_completed,
                    provider_session_id.as_deref(),
                    &session_context.canonical,
                ),
            )?;
            return Ok(());
        }

        let turn_number = turns_completed + 1;
        let turn_plan = plan_execution_turn(&turn_context, &issue, &session_context, turn_number);
        if let Err(seed_error) = seed_execution_turn_canonical_metadata(
            &issue,
            turn_number,
            &turn_context,
            turn_plan,
            ExecutionTurnDelta {
                previous_review: last_review.as_ref(),
                verification_summary: session_context.verification_summary.as_ref(),
            },
            session_context.latest_resume_handle.as_ref(),
            &mut session_context.canonical,
        ) {
            eprintln!(
                "warning: failed to seed canonical listen metadata for {} turn {turn_number} before persisting running state: {seed_error:#}",
                issue.identifier,
            );
        }
        let snapshot_before = capture_workspace_snapshot(&workspace_path, &args.issue)?;
        write_listen_session(
            &source_root,
            project_selector,
            build_worker_session(
                &issue,
                SessionPhase::Running,
                compact_running_summary(
                    issue.description.as_deref(),
                    turn_number,
                    turn_plan.max_turns,
                    0,
                ),
                &session_context,
                turns_completed,
                provider_session_id.as_deref(),
                &session_context.canonical,
            ),
        )?;

        // Determine whether this turn will actually attempt a resumed invocation.
        // Only retry on failure when resume was genuinely attempted (not just "handle exists").
        let attempted_resume = turn_number > 1
            && session_context
                .latest_resume_handle
                .as_ref()
                .is_some_and(|h| {
                    resolve_effective_listen_agent(
                        &app_config,
                        &planning_meta,
                        args.agent.as_deref(),
                    )
                    .as_deref()
                    .is_some_and(|a| h.matches_agent(a))
                });

        // Keep provider-native manual resume handles separate from provider session bookkeeping.
        let provider_session_id_state = RefCell::new(provider_session_id.clone());
        let turn_result = match execute_agent_turn(
            &issue,
            turn_number,
            &turn_context,
            turn_plan,
            ExecutionTurnDelta {
                previous_review: last_review.as_ref(),
                verification_summary: session_context.verification_summary.as_ref(),
            },
            session_context.latest_resume_handle.as_ref(),
            |current_session_id| {
                if provider_session_id_state.borrow().as_deref() == Some(current_session_id) {
                    return Ok(());
                }
                *provider_session_id_state.borrow_mut() = Some(current_session_id.to_string());
                write_listen_session(
                    &source_root,
                    project_selector,
                    build_worker_session(
                        &issue,
                        SessionPhase::Running,
                        compact_running_summary(
                            issue.description.as_deref(),
                            turn_number,
                            turn_plan.max_turns,
                            0,
                        ),
                        &session_context,
                        turns_completed,
                        provider_session_id_state.borrow().as_deref(),
                        &session_context.canonical,
                    ),
                )
            },
            |usage| {
                let mut displayed_tokens = session_tokens.clone();
                let mut displayed_canonical = session_context.canonical.clone();
                displayed_tokens.accumulate(&TokenUsage {
                    input: usage.input,
                    output: usage.output,
                });
                displayed_canonical.tokens = displayed_tokens.clone();
                write_listen_session(
                    &source_root,
                    project_selector,
                    build_worker_session(
                        &issue,
                        SessionPhase::Running,
                        compact_running_summary(
                            issue.description.as_deref(),
                            turn_number,
                            turn_plan.max_turns,
                            0,
                        ),
                        &session_context,
                        turns_completed,
                        provider_session_id_state.borrow().as_deref(),
                        &displayed_canonical,
                    ),
                )
            },
        ) {
            Ok(result) => result,
            Err(error)
                if attempted_resume
                    && resolve_effective_listen_agent(
                        &app_config,
                        &planning_meta,
                        args.agent.as_deref(),
                    )
                    .and_then(|agent| crate::agent_provider::builtin_provider_adapter(&agent))
                    .is_some_and(|provider| {
                        provider.is_invalid_resume_error(&error.to_string())
                    }) =>
            {
                eprintln!(
                    "listen: invalid resume for {} turn {turn_number}, retrying as cold start: {error}",
                    issue.identifier,
                );
                session_context.latest_resume_handle = None;
                let provider_session_id_retry = RefCell::new(provider_session_id.clone());
                match execute_agent_turn(
                    &issue,
                    turn_number,
                    &turn_context,
                    turn_plan,
                    ExecutionTurnDelta {
                        previous_review: last_review.as_ref(),
                        verification_summary: session_context.verification_summary.as_ref(),
                    },
                    None,
                    |current_session_id| {
                        if provider_session_id_retry.borrow().as_deref() == Some(current_session_id)
                        {
                            return Ok(());
                        }
                        *provider_session_id_retry.borrow_mut() =
                            Some(current_session_id.to_string());
                        write_listen_session(
                            &source_root,
                            project_selector,
                            build_worker_session(
                                &issue,
                                SessionPhase::Running,
                                compact_running_summary(
                                    issue.description.as_deref(),
                                    turn_number,
                                    turn_plan.max_turns,
                                    0,
                                ),
                                &session_context,
                                turns_completed,
                                provider_session_id_retry.borrow().as_deref(),
                                &session_context.canonical,
                            ),
                        )
                    },
                    |usage| {
                        let mut displayed_tokens = session_tokens.clone();
                        let mut displayed_canonical = session_context.canonical.clone();
                        displayed_tokens.accumulate(&TokenUsage {
                            input: usage.input,
                            output: usage.output,
                        });
                        displayed_canonical.tokens = displayed_tokens.clone();
                        write_listen_session(
                            &source_root,
                            project_selector,
                            build_worker_session(
                                &issue,
                                SessionPhase::Running,
                                compact_running_summary(
                                    issue.description.as_deref(),
                                    turn_number,
                                    turn_plan.max_turns,
                                    0,
                                ),
                                &session_context,
                                turns_completed,
                                provider_session_id_retry.borrow().as_deref(),
                                &displayed_canonical,
                            ),
                        )
                    },
                ) {
                    Ok(result) => {
                        // Sync the retry provider session ID back so the outer into_inner picks it up.
                        *provider_session_id_state.borrow_mut() =
                            provider_session_id_retry.into_inner();
                        result
                    }
                    Err(retry_error) => {
                        if let Err(seed_error) = seed_execution_turn_canonical_metadata(
                            &issue,
                            turn_number,
                            &turn_context,
                            turn_plan,
                            ExecutionTurnDelta {
                                previous_review: last_review.as_ref(),
                                verification_summary: session_context.verification_summary.as_ref(),
                            },
                            None,
                            &mut session_context.canonical,
                        ) {
                            eprintln!(
                                "warning: failed to seed canonical listen metadata for {} turn {turn_number} before persisting resume-retry failure: {seed_error:#}",
                                issue.identifier,
                            );
                        }
                        let blocked = blocked_reason(
                            BlockedCategory::Turn,
                            format!(
                                "turn {turn_number}/{} failed (resume retry)",
                                args.max_turns
                            ),
                            true,
                        );
                        write_listen_session(
                            &source_root,
                            project_selector,
                            build_worker_blocked_session(
                                &issue,
                                blocked.clone(),
                                compact_blocked_summary(
                                    &blocked,
                                    issue.description.as_deref(),
                                    &log_path,
                                ),
                                &session_context,
                                turns_completed,
                                provider_session_id.as_deref(),
                                &session_context.canonical,
                            ),
                        )?;
                        return Err(retry_error);
                    }
                }
            }
            Err(error) => {
                if let Err(seed_error) = seed_execution_turn_canonical_metadata(
                    &issue,
                    turn_number,
                    &turn_context,
                    turn_plan,
                    ExecutionTurnDelta {
                        previous_review: last_review.as_ref(),
                        verification_summary: session_context.verification_summary.as_ref(),
                    },
                    session_context.latest_resume_handle.as_ref(),
                    &mut session_context.canonical,
                ) {
                    eprintln!(
                        "warning: failed to seed canonical listen metadata for {} turn {turn_number} before persisting turn failure: {seed_error:#}",
                        issue.identifier,
                    );
                }
                let blocked = blocked_reason(
                    BlockedCategory::Turn,
                    format!("turn {turn_number}/{} failed", args.max_turns),
                    true,
                );
                write_listen_session(
                    &source_root,
                    project_selector,
                    build_worker_blocked_session(
                        &issue,
                        blocked.clone(),
                        compact_blocked_summary(&blocked, issue.description.as_deref(), &log_path),
                        &session_context,
                        turns_completed,
                        provider_session_id.as_deref(),
                        &session_context.canonical,
                    ),
                )?;
                return Err(error);
            }
        };
        session_context.last_timeout = turn_result.timeout.clone();
        update_resume_handle_after_turn(
            &mut session_context,
            &turn_result,
            turn_plan,
            &issue.identifier,
            turn_number,
        );
        provider_session_id = turn_result
            .session_id
            .or_else(|| provider_session_id_state.into_inner());
        if let Some(provider) = turn_result.provider {
            session_context.canonical.provider = Some(provider);
            session_context.canonical.model = turn_result.model;
            session_context.canonical.reasoning = turn_result.reasoning;
        }
        let turn_snapshot = TurnTokenSnapshot {
            turn: turn_number,
            prompt_mode: turn_result.prompt_mode,
            tokens: turn_result
                .usage
                .as_ref()
                .map(|usage| TokenUsage {
                    input: usage.input,
                    output: usage.output,
                })
                .unwrap_or_default(),
            captured_at_epoch_seconds: now_epoch_seconds(),
        };
        append_turn_token_summary(&log_path, &turn_snapshot)?;
        if let Some(existing) = session_context
            .turn_history
            .iter_mut()
            .find(|snapshot| snapshot.turn == turn_snapshot.turn)
        {
            *existing = turn_snapshot.clone();
        } else {
            session_context.turn_history.push(turn_snapshot);
        }
        if let Some(usage) = turn_result.usage {
            session_tokens.accumulate(&TokenUsage {
                input: usage.input,
                output: usage.output,
            });
        }
        session_context.canonical.tokens = session_tokens.clone();

        turns_completed = turn_number;
        let snapshot_after = capture_workspace_snapshot(&workspace_path, &args.issue)?;
        let turn_progress =
            compare_workspace_snapshots(&workspace_path, &snapshot_before, &snapshot_after)?;
        match load_worker_issue(&service, &args.issue).await {
            Ok(refreshed_issue) => {
                issue = refreshed_issue;
            }
            Err(error) => {
                defer_pending_linear_sync_operation(
                    &app_config,
                    &mut session_context.pending_linear_sync,
                    &error,
                    "issue refresh",
                    &log_path,
                    |pending| {
                        pending.require_issue_refresh = true;
                    },
                )?;
                write_pending_linear_sync_blocked_session(
                    &issue,
                    &session_context,
                    turns_completed,
                    provider_session_id.as_deref(),
                    &log_path,
                )?;
                return Ok(());
            }
        }
        let context_checkpoint =
            (turn_plan.checkpoint_turn && turn_result.timeout.is_none()).then(|| {
                build_context_checkpoint(
                    &issue,
                    &turn_context,
                    &session_context,
                    turn_number,
                    turn_plan,
                    &snapshot_after.status_entries,
                )
            });

        if !listen_issue_is_active(issue.state.as_ref().map(|state| state.name.as_str())) {
            continue;
        }

        if let Some(timeout) = turn_result.timeout.clone() {
            let review = review_for_turn_timeout(&timeout);
            write_listen_session(
                &source_root,
                project_selector,
                build_worker_session(
                    &issue,
                    SessionPhase::Running,
                    compact_session_summary([
                        Some(format!("Timeout | {}", timeout.summary_label())),
                        Some(super::ticket_progress_summary(issue.description.as_deref())),
                        Some(format!(
                            "{} turn(s) remaining",
                            turn_plan.remaining_execution_turns_after_current(turn_number)
                        )),
                    ]),
                    &session_context,
                    turns_completed,
                    provider_session_id.as_deref(),
                    &session_context.canonical,
                ),
            )?;
            sync_review_tracking_with_checkpoint(
                &service,
                &issue,
                &turn_context,
                &app_config,
                &mut session_context,
                &log_path,
                &review,
                context_checkpoint.as_deref(),
            )
            .await?;
            last_review = Some(review);
            if turn_plan.last_execution_turn {
                return block_for_context_pressure_limit(
                    &source_root,
                    project_selector,
                    &issue,
                    &session_context,
                    turns_completed,
                    provider_session_id.as_deref(),
                    &log_path,
                );
            }
            if turns_completed >= args.max_turns {
                let blocked = blocked_reason(BlockedCategory::Turn, timeout.summary_label(), false);
                write_listen_session(
                    &source_root,
                    project_selector,
                    build_worker_blocked_session(
                        &issue,
                        blocked.clone(),
                        compact_session_summary([
                            Some(blocked.summary_headline()),
                            Some("turn budget exhausted".to_string()),
                            Some(format!("see {}", log_path.display())),
                        ]),
                        &session_context,
                        turns_completed,
                        provider_session_id.as_deref(),
                        &session_context.canonical,
                    ),
                )?;
                return Ok(());
            }
            continue;
        }

        let backlog_progress = backlog_issue
            .as_ref()
            .map(|backlog_issue| {
                backlog_progress_for_issue_dir(&workspace_path, &backlog_issue.identifier)
            })
            .transpose()?;
        let meaningful_turn_progress =
            turn_progress.implementation_changed() || turn_progress.planning_changed();
        let review = run_review_phase(
            &issue,
            turn_number,
            meaningful_turn_progress,
            &turn_progress,
            &turn_context,
            WorkerPhaseContext {
                source_root: &source_root,
                project_selector,
                session_context: &session_context,
                provider_session_id: provider_session_id.as_deref(),
                log_path: &log_path,
                previous_review: last_review.as_ref(),
            },
        )
        .await?;
        sync_review_tracking_with_checkpoint(
            &service,
            &issue,
            &turn_context,
            &app_config,
            &mut session_context,
            &log_path,
            &review,
            context_checkpoint.as_deref(),
        )
        .await?;
        last_review = Some(review.clone());
        if meaningful_turn_progress {
            stalled_turns = 0;
        } else {
            stalled_turns += 1;
        }

        if review.complete {
            let final_review = run_final_review_phase(
                &issue,
                turn_number,
                &turn_context,
                &review,
                WorkerPhaseContext {
                    source_root: &source_root,
                    project_selector,
                    session_context: &session_context,
                    provider_session_id: provider_session_id.as_deref(),
                    log_path: &log_path,
                    previous_review: None,
                },
            )
            .await?;
            if final_review.approved {
                if let Some(branch) = branch.as_deref() {
                    let pull_request = match publish_listener_pull_request(
                        &issue,
                        &workspace_path,
                        branch,
                        PullRequestPublishMode::Draft,
                        Some(&review),
                        session_context.verification_summary.as_ref(),
                    )
                    .await
                    {
                        Ok(pull_request) => pull_request,
                        Err(error) => {
                            let blocked = blocked_reason(
                                BlockedCategory::Infra,
                                "failed to prepare GitHub PR for verification",
                                true,
                            );
                            write_listen_session(
                                &source_root,
                                project_selector,
                                build_worker_blocked_session(
                                    &issue,
                                    blocked.clone(),
                                    compact_blocked_summary(
                                        &blocked,
                                        issue.description.as_deref(),
                                        &log_path,
                                    ),
                                    &session_context,
                                    turns_completed,
                                    provider_session_id.as_deref(),
                                    &session_context.canonical,
                                ),
                            )?;
                            return Err(error);
                        }
                    };
                    if let Some(pull_request) = pull_request.as_ref() {
                        session_context.pull_request =
                            PullRequestSummary::from(pull_request.clone());
                        if let Err(error) =
                            ensure_listener_pull_request_attachment(&service, &issue, pull_request)
                                .await
                        {
                            defer_pending_linear_sync_operation(
                                &app_config,
                                &mut session_context.pending_linear_sync,
                                &error,
                                "pull request attachment",
                                &log_path,
                                |pending| {
                                    pending.pull_request_attachment =
                                        Some(PendingPullRequestAttachment {
                                            number: pull_request.number,
                                            url: pull_request.url.clone(),
                                        });
                                },
                            )?;
                        }
                    }
                }
                let verification = run_verification_phase(
                    &issue,
                    turn_number,
                    &turn_context,
                    WorkerPhaseContext {
                        source_root: &source_root,
                        project_selector,
                        session_context: &session_context,
                        provider_session_id: provider_session_id.as_deref(),
                        log_path: &log_path,
                        previous_review: None,
                    },
                )
                .await?;
                session_context.verification_summary = Some(verification.summary_snapshot());
                sync_review_tracking(
                    &service,
                    &issue,
                    &turn_context,
                    &app_config,
                    &mut session_context,
                    &log_path,
                    &review,
                )
                .await?;
                if verification.status == VerificationStatus::Failed {
                    let budget_exhausted = remaining_verification_repair_turns == 0;
                    let remaining_after_failure =
                        remaining_verification_repair_turns.saturating_sub(1);
                    let follow_up_review = review_for_verification_failure(
                        &review,
                        &verification,
                        remaining_after_failure,
                    );
                    sync_review_tracking(
                        &service,
                        &issue,
                        &turn_context,
                        &app_config,
                        &mut session_context,
                        &log_path,
                        &follow_up_review,
                    )
                    .await?;
                    last_review = Some(follow_up_review);
                    if budget_exhausted {
                        let blocked = blocked_reason(
                            BlockedCategory::Gate,
                            "verification failed and repair budget exhausted",
                            false,
                        );
                        write_listen_session(
                            &source_root,
                            project_selector,
                            build_worker_blocked_session(
                                &issue,
                                blocked.clone(),
                                compact_blocked_summary(
                                    &blocked,
                                    issue.description.as_deref(),
                                    &log_path,
                                ),
                                &session_context,
                                turns_completed,
                                provider_session_id.as_deref(),
                                &session_context.canonical,
                            ),
                        )?;
                        return Ok(());
                    }
                    if turn_plan.last_execution_turn {
                        return block_for_context_pressure_limit(
                            &source_root,
                            project_selector,
                            &issue,
                            &session_context,
                            turns_completed,
                            provider_session_id.as_deref(),
                            &log_path,
                        );
                    }
                    remaining_verification_repair_turns -= 1;
                    continue;
                }
                let review = match run_pre_pr_validation_gate(
                    PrePrValidationGateContext {
                        issue: &issue,
                        turn_context: &turn_context,
                        phase_context: WorkerPhaseContext {
                            source_root: &source_root,
                            project_selector,
                            session_context: &session_context,
                            provider_session_id: provider_session_id.as_deref(),
                            log_path: &log_path,
                            previous_review: None,
                        },
                        turns_completed,
                        pr_mutation_description: "review-ready PR promotion",
                    },
                    &review,
                    &mut remaining_validation_repair_turns,
                )
                .await?
                {
                    ValidationGateOutcome::Passed(review) => review,
                    ValidationGateOutcome::Retry(review) => {
                        sync_review_tracking(
                            &service,
                            &issue,
                            &turn_context,
                            &app_config,
                            &mut session_context,
                            &log_path,
                            &review,
                        )
                        .await?;
                        last_review = Some(review);
                        if turn_plan.last_execution_turn {
                            return block_for_context_pressure_limit(
                                &source_root,
                                project_selector,
                                &issue,
                                &session_context,
                                turns_completed,
                                provider_session_id.as_deref(),
                                &log_path,
                            );
                        }
                        continue;
                    }
                    ValidationGateOutcome::Exhausted(review) => {
                        sync_review_tracking(
                            &service,
                            &issue,
                            &turn_context,
                            &app_config,
                            &mut session_context,
                            &log_path,
                            &review,
                        )
                        .await?;
                        return Ok(());
                    }
                };
                write_listen_session(
                    &source_root,
                    project_selector,
                    build_worker_session(
                        &issue,
                        SessionPhase::Publishing,
                        compact_session_summary([
                            Some("Publishing review-ready handoff".to_string()),
                            Some(format!("see {}", log_path.display())),
                        ]),
                        &session_context,
                        turns_completed,
                        provider_session_id.as_deref(),
                        &session_context.canonical,
                    ),
                )?;
                let branch = branch.as_deref().ok_or_else(|| {
                    anyhow!("failed to inspect the workspace branch before promoting the review PR")
                })?;
                let pull_request = match prepare_listener_pull_request_for_review(
                    &issue,
                    &workspace_path,
                    branch,
                    &session_context.pull_request,
                    &review,
                    session_context.verification_summary.as_ref(),
                )
                .await
                {
                    Ok(pull_request) => pull_request,
                    Err(error) => {
                        let blocked = blocked_reason(
                            BlockedCategory::Infra,
                            "failed to prepare GitHub PR for review",
                            true,
                        );
                        write_listen_session(
                            &source_root,
                            project_selector,
                            build_worker_blocked_session(
                                &issue,
                                blocked.clone(),
                                compact_blocked_summary(
                                    &blocked,
                                    issue.description.as_deref(),
                                    &log_path,
                                ),
                                &session_context,
                                turns_completed,
                                provider_session_id.as_deref(),
                                &session_context.canonical,
                            ),
                        )?;
                        return Err(error);
                    }
                };
                session_context.pull_request = pull_request
                    .clone()
                    .map(PullRequestSummary::from)
                    .unwrap_or_default();
                if let Some(pull_request) = pull_request.as_ref()
                    && let Err(error) =
                        ensure_listener_pull_request_attachment(&service, &issue, pull_request)
                            .await
                {
                    defer_pending_linear_sync_operation(
                        &app_config,
                        &mut session_context.pending_linear_sync,
                        &error,
                        "pull request attachment",
                        &log_path,
                        |pending| {
                            pending.pull_request_attachment = Some(PendingPullRequestAttachment {
                                number: pull_request.number,
                                url: pull_request.url.clone(),
                            });
                        },
                    )?;
                    write_pending_linear_sync_blocked_session(
                        &issue,
                        &session_context,
                        turns_completed,
                        provider_session_id.as_deref(),
                        &log_path,
                    )?;
                    return Ok(());
                }
                if let Some(number) = session_context.pull_request.number {
                    let gate_context = PostPublicationCiGateContext {
                        issue: &issue,
                        workspace_path: &workspace_path,
                        review: &review,
                        summary_prefix: "Publishing review-ready handoff",
                        turn_context: &turn_context,
                        turns_completed,
                        provider_session_id: provider_session_id.as_deref(),
                        log_path: &log_path,
                        service: &service,
                    };
                    match run_post_publication_ci_settle_gate(
                        number,
                        &gate_context,
                        &mut session_context,
                        &mut remaining_validation_repair_turns,
                    )
                    .await?
                    {
                        PostPublicationCiGateOutcome::Passed => {}
                        PostPublicationCiGateOutcome::Retry(follow_up_review) => {
                            last_review = Some(follow_up_review);
                            if turn_plan.last_execution_turn {
                                return block_for_context_pressure_limit(
                                    &source_root,
                                    project_selector,
                                    &issue,
                                    &session_context,
                                    turns_completed,
                                    provider_session_id.as_deref(),
                                    &log_path,
                                );
                            }
                            continue;
                        }
                        PostPublicationCiGateOutcome::Blocked => return Ok(()),
                    }
                }
                let transitioned_issue =
                    match try_transition_issue_to_review_state(&service, &issue).await {
                        Ok(transitioned_issue) => transitioned_issue,
                        Err(error) => {
                            defer_pending_linear_sync_operation(
                                &app_config,
                                &mut session_context.pending_linear_sync,
                                &error,
                                "issue review transition",
                                &log_path,
                                |pending| {
                                    pending.review_transition_issue = true;
                                },
                            )?;
                            write_pending_linear_sync_blocked_session(
                                &issue,
                                &session_context,
                                turns_completed,
                                provider_session_id.as_deref(),
                                &log_path,
                            )?;
                            return Ok(());
                        }
                    };
                if let Some(backlog_issue) = backlog_issue.as_ref()
                    && !backlog_issue
                        .identifier
                        .eq_ignore_ascii_case(&issue.identifier)
                {
                    if let Err(error) =
                        try_transition_issue_to_review_state(&service, backlog_issue).await
                    {
                        defer_pending_linear_sync_operation(
                            &app_config,
                            &mut session_context.pending_linear_sync,
                            &error,
                            "backlog review transition",
                            &log_path,
                            |pending| {
                                pending.review_transition_backlog_issue =
                                    Some(backlog_issue.identifier.clone());
                            },
                        )?;
                        write_pending_linear_sync_blocked_session(
                            &issue,
                            &session_context,
                            turns_completed,
                            provider_session_id.as_deref(),
                            &log_path,
                        )?;
                        return Ok(());
                    }
                }
                let refreshed_issue = load_worker_issue(&service, &args.issue)
                    .await
                    .unwrap_or_else(|_| {
                        transitioned_issue.clone().unwrap_or_else(|| issue.clone())
                    });
                let review_transition_applied = !listen_issue_is_active(
                    refreshed_issue
                        .state
                        .as_ref()
                        .map(|state| state.name.as_str()),
                );

                if review_transition_applied {
                    let summary = compact_session_summary([
                        Some("Complete".to_string()),
                        Some(super::ticket_progress_summary(
                            refreshed_issue.description.as_deref(),
                        )),
                        Some(format!("{turns_completed} turn(s)")),
                        Some(format!(
                            "moved to `{}`",
                            issue_state_label(&refreshed_issue)
                        )),
                        session_context.github_ci_summary.clone(),
                    ]);
                    write_listen_session(
                        &source_root,
                        project_selector,
                        build_worker_session(
                            &refreshed_issue,
                            SessionPhase::Completed,
                            summary,
                            &session_context,
                            turns_completed,
                            provider_session_id.as_deref(),
                            &session_context.canonical,
                        ),
                    )?;
                    try_listener_auto_clean(
                        &source_root,
                        project_selector,
                        &workspace_path,
                        &args.issue,
                    );
                    return Ok(());
                }

                update_pending_linear_sync(&mut session_context.pending_linear_sync, |pending| {
                    pending.review_transition_issue = true;
                });
                append_worker_log(
                    &log_path,
                    "pending linear sync",
                    &[
                        "Deferred review transition replay without a captured Linear error"
                            .to_string(),
                    ],
                )?;
                write_pending_linear_sync_blocked_session(
                    &refreshed_issue,
                    &session_context,
                    turns_completed,
                    provider_session_id.as_deref(),
                    &log_path,
                )?;
                return Ok(());
            }
            let follow_up_review = ReviewReport {
                summary: final_review.summary.clone(),
                complete: false,
                completed_items: review.completed_items.clone(),
                remaining_items: final_review.missing_items.clone(),
                validation_completed: review.validation_completed.clone(),
                validation_remaining: final_review.validation_gaps.clone(),
                risks: final_review.risks.clone(),
                notes: final_review.notes.clone(),
            };
            sync_review_tracking(
                &service,
                &issue,
                &turn_context,
                &app_config,
                &mut session_context,
                &log_path,
                &follow_up_review,
            )
            .await?;
            last_review = Some(follow_up_review);
        }

        if backlog_progress.is_some() {
            if let Some(branch) = branch.as_deref() {
                let review = last_review.as_ref().ok_or_else(|| {
                    anyhow!("listen review tracking was unavailable before draft PR publication")
                })?;
                let review = match run_pre_pr_validation_gate(
                    PrePrValidationGateContext {
                        issue: &issue,
                        turn_context: &turn_context,
                        phase_context: WorkerPhaseContext {
                            source_root: &source_root,
                            project_selector,
                            session_context: &session_context,
                            provider_session_id: provider_session_id.as_deref(),
                            log_path: &log_path,
                            previous_review: None,
                        },
                        turns_completed,
                        pr_mutation_description: "draft PR publication",
                    },
                    review,
                    &mut remaining_validation_repair_turns,
                )
                .await?
                {
                    ValidationGateOutcome::Passed(review) => review,
                    ValidationGateOutcome::Retry(review) => {
                        sync_review_tracking(
                            &service,
                            &issue,
                            &turn_context,
                            &app_config,
                            &mut session_context,
                            &log_path,
                            &review,
                        )
                        .await?;
                        last_review = Some(review);
                        if turn_plan.last_execution_turn {
                            return block_for_context_pressure_limit(
                                &source_root,
                                project_selector,
                                &issue,
                                &session_context,
                                turns_completed,
                                provider_session_id.as_deref(),
                                &log_path,
                            );
                        }
                        continue;
                    }
                    ValidationGateOutcome::Exhausted(review) => {
                        sync_review_tracking(
                            &service,
                            &issue,
                            &turn_context,
                            &app_config,
                            &mut session_context,
                            &log_path,
                            &review,
                        )
                        .await?;
                        return Ok(());
                    }
                };
                match publish_listener_pull_request(
                    &issue,
                    &workspace_path,
                    branch,
                    PullRequestPublishMode::Draft,
                    Some(&review),
                    session_context.verification_summary.as_ref(),
                )
                .await
                {
                    Ok(Some(pull_request)) => {
                        session_context.pull_request =
                            PullRequestSummary::from(pull_request.clone());
                        if let Err(error) =
                            ensure_listener_pull_request_attachment(&service, &issue, &pull_request)
                                .await
                        {
                            defer_pending_linear_sync_operation(
                                &app_config,
                                &mut session_context.pending_linear_sync,
                                &error,
                                "pull request attachment",
                                &log_path,
                                |pending| {
                                    pending.pull_request_attachment =
                                        Some(PendingPullRequestAttachment {
                                            number: pull_request.number,
                                            url: pull_request.url.clone(),
                                        });
                                },
                            )?;
                        }
                        if let Some(number) = session_context.pull_request.number {
                            let gate_context = PostPublicationCiGateContext {
                                issue: &issue,
                                workspace_path: &workspace_path,
                                review: &review,
                                summary_prefix: "Publishing draft PR",
                                turn_context: &turn_context,
                                turns_completed,
                                provider_session_id: provider_session_id.as_deref(),
                                log_path: &log_path,
                                service: &service,
                            };
                            match run_post_publication_ci_settle_gate(
                                number,
                                &gate_context,
                                &mut session_context,
                                &mut remaining_validation_repair_turns,
                            )
                            .await?
                            {
                                PostPublicationCiGateOutcome::Passed => {}
                                PostPublicationCiGateOutcome::Retry(follow_up_review) => {
                                    last_review = Some(follow_up_review);
                                    if turn_plan.last_execution_turn {
                                        return block_for_context_pressure_limit(
                                            &source_root,
                                            project_selector,
                                            &issue,
                                            &session_context,
                                            turns_completed,
                                            provider_session_id.as_deref(),
                                            &log_path,
                                        );
                                    }
                                    continue;
                                }
                                PostPublicationCiGateOutcome::Blocked => return Ok(()),
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(err) => {
                        eprintln!("draft PR publish failed; continuing worker loop: {err:#}");
                    }
                }
            }

            if stalled_turns >= MAX_STALLED_TURNS {
                let blocked = blocked_reason(
                    BlockedCategory::Turn,
                    format!("stalled after {stalled_turns} turn(s)"),
                    false,
                );
                write_listen_session(
                    &source_root,
                    project_selector,
                    build_worker_blocked_session(
                        &issue,
                        blocked.clone(),
                        compact_blocked_summary(&blocked, issue.description.as_deref(), &log_path),
                        &session_context,
                        turns_completed,
                        provider_session_id.as_deref(),
                        &session_context.canonical,
                    ),
                )?;
                return Ok(());
            }

            if turn_plan.last_execution_turn {
                return block_for_context_pressure_limit(
                    &source_root,
                    project_selector,
                    &issue,
                    &session_context,
                    turns_completed,
                    provider_session_id.as_deref(),
                    &log_path,
                );
            }

            write_listen_session(
                &source_root,
                project_selector,
                build_worker_session(
                    &issue,
                    SessionPhase::Running,
                    compact_running_summary(
                        issue.description.as_deref(),
                        turns_completed,
                        turn_plan.max_turns,
                        stalled_turns,
                    ),
                    &session_context,
                    turns_completed,
                    provider_session_id.as_deref(),
                    &session_context.canonical,
                ),
            )?;
        } else {
            if turn_plan.last_execution_turn {
                return block_for_context_pressure_limit(
                    &source_root,
                    project_selector,
                    &issue,
                    &session_context,
                    turns_completed,
                    provider_session_id.as_deref(),
                    &log_path,
                );
            }
            write_listen_session(
                &source_root,
                project_selector,
                build_worker_session(
                    &issue,
                    SessionPhase::Running,
                    compact_running_summary(
                        issue.description.as_deref(),
                        turns_completed,
                        turn_plan.max_turns,
                        stalled_turns,
                    ),
                    &session_context,
                    turns_completed,
                    provider_session_id.as_deref(),
                    &session_context.canonical,
                ),
            )?;
        }
    }
}

/// Best-effort auto-clean for a listener workspace after the session completes.
///
/// When the workspace is safe (clean git state, within expected sibling root), removes the
/// workspace clone and its ticket-scoped listen artifacts (session entry, detail, log). When the
/// workspace has uncommitted changes, unpushed commits, or other safety risks, logs the skip
/// reason and leaves the workspace in place for manual cleanup via `meta workspace prune`.
fn try_listener_auto_clean(
    source_root: &Path,
    project_selector: Option<&str>,
    workspace_path: &Path,
    issue_identifier: &str,
) {
    let workspace_root = match sibling_workspace_root(source_root) {
        Ok(root) => root,
        Err(error) => {
            eprintln!(
                "listen: auto-clean skipped for {issue_identifier}: \
                 failed to resolve workspace root: {error}"
            );
            return;
        }
    };

    match try_auto_clean_workspace(
        source_root,
        project_selector,
        &workspace_root,
        workspace_path,
        issue_identifier,
    ) {
        Ok(AutoCleanOutcome::Removed { bytes_reclaimed }) => {
            eprintln!(
                "listen: auto-cleaned workspace for {issue_identifier} \
                 (freed {} bytes)",
                bytes_reclaimed
            );
        }
        Ok(AutoCleanOutcome::Skipped { reason }) => {
            eprintln!(
                "listen: auto-clean skipped for {issue_identifier}: \
                 {reason} (manual review needed)"
            );
        }
        Err(error) => {
            eprintln!("listen: auto-clean failed for {issue_identifier}: {error:#}");
        }
    }
}

struct ListenTurnContext<'a> {
    app_config: &'a AppConfig,
    planning_meta: &'a crate::config::PlanningMeta,
    args: &'a ListenWorkerArgs,
    source_root: &'a Path,
    project_selector: Option<&'a str>,
    workspace_path: &'a Path,
    workpad_comment_id: &'a str,
    backlog_issue: Option<&'a IssueSummary>,
    max_turns: u32,
    context_budget_tokens: u64,
}

struct WorkerSessionContext<'a> {
    source_root: &'a Path,
    project_selector: Option<&'a str>,
    workspace_path: &'a Path,
    branch: Option<&'a str>,
    workpad_comment_id: &'a str,
    backlog_issue: Option<&'a IssueSummary>,
    pid: Option<u32>,
    latest_resume_handle: Option<LatestResumeHandle>,
    context_budget_tokens: u64,
    pending_linear_sync: Option<PendingLinearSync>,
    started_at_epoch_seconds: u64,
    stale_worker_recovery_attempt_count: u32,
    latest_stale_worker_failure: Option<super::StaleWorkerFailure>,
    last_timeout: Option<SessionTimeoutRecord>,
    turn_history: Vec<TurnTokenSnapshot>,
    canonical: CanonicalSessionData,
    pull_request: PullRequestSummary,
    github_ci_summary: Option<String>,
    verification_summary: Option<VerificationSummary>,
    origin: super::state::SessionOrigin,
}

struct AgentPhaseInvocation<'a> {
    issue: &'a IssueSummary,
    context: &'a ListenTurnContext<'a>,
    turn_number: u32,
    phase_label: &'a str,
    prompt_mode: TurnPromptMode,
    capture_response_text: bool,
    continuation_handle: Option<&'a LatestResumeHandle>,
}

struct WorkerPhaseContext<'a> {
    source_root: &'a Path,
    project_selector: Option<&'a str>,
    session_context: &'a WorkerSessionContext<'a>,
    provider_session_id: Option<&'a str>,
    log_path: &'a Path,
    previous_review: Option<&'a ReviewReport>,
}

#[derive(Debug, Default)]
struct TurnExecutionResult {
    session_id: Option<String>,
    usage: Option<AgentTokenUsage>,
    latest_resume_handle: Option<LatestResumeHandle>,
    timeout: Option<SessionTimeoutRecord>,
    prompt_mode: TurnPromptMode,
    provider: Option<String>,
    model: Option<String>,
    reasoning: Option<String>,
    response_text: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct ExecutionTurnPlan {
    pressure: ContextPressure,
    known_input_tokens: u64,
    checkpoint_turn: bool,
    last_execution_turn: bool,
    max_turns: u32,
}

impl ExecutionTurnPlan {
    fn new(
        pressure: ContextPressure,
        known_input_tokens: u64,
        checkpoint_turn: bool,
        last_execution_turn: bool,
        turn_number: u32,
        max_turns: u32,
    ) -> Self {
        Self {
            pressure,
            known_input_tokens,
            checkpoint_turn,
            last_execution_turn,
            max_turns: if last_execution_turn {
                turn_number
            } else {
                max_turns
            },
        }
    }

    fn remaining_execution_turns_after_current(self, turn_number: u32) -> u32 {
        self.max_turns.saturating_sub(turn_number)
    }
}

fn plan_execution_turn(
    context: &ListenTurnContext<'_>,
    issue: &IssueSummary,
    session_context: &WorkerSessionContext<'_>,
    turn_number: u32,
) -> ExecutionTurnPlan {
    let known_input_tokens = completed_turn_known_input_tokens(&session_context.turn_history);
    let pressure = ContextPressure::from_turn_history(
        &session_context.turn_history,
        context.context_budget_tokens,
    );
    let effective_workpad = effective_workpad_body(
        issue,
        session_context.pending_linear_sync.as_ref(),
        context.workpad_comment_id,
    );
    let checkpoint_turn = matches!(pressure, ContextPressure::High | ContextPressure::Critical)
        && !effective_workpad
            .as_deref()
            .is_some_and(context_checkpoint_present);
    ExecutionTurnPlan::new(
        pressure,
        known_input_tokens,
        checkpoint_turn,
        pressure == ContextPressure::Critical,
        turn_number,
        context.max_turns,
    )
}

fn build_context_checkpoint(
    issue: &IssueSummary,
    context: &ListenTurnContext<'_>,
    session_context: &WorkerSessionContext<'_>,
    turn_number: u32,
    plan: ExecutionTurnPlan,
    workspace_status_entries: &[String],
) -> String {
    let effective_body = effective_workpad_body(
        issue,
        session_context.pending_linear_sync.as_ref(),
        context.workpad_comment_id,
    );
    let completed = effective_body
        .as_deref()
        .map(|body| extract_workpad_section_lines(body, "### Completed"))
        .unwrap_or_default();
    let remaining = effective_body
        .as_deref()
        .map(|body| extract_workpad_section_lines(body, "### Remaining"))
        .unwrap_or_default();
    let validation = effective_body
        .as_deref()
        .map(|body| extract_workpad_section_lines(body, "### Validation"))
        .unwrap_or_default();
    let known_input_tokens = completed_turn_known_input_tokens(&session_context.turn_history);

    let mut lines = vec![
        "#### Context Checkpoint".to_string(),
        String::new(),
        format!("- Captured at: {}", now_timestamp()),
        format!("- Turns completed: {turn_number}"),
        format!("- Session phase: {}", SessionPhase::Running.display_label()),
        format!("- Context pressure: {}", plan.pressure.label()),
        format!(
            "- Known input tokens: {} / {}",
            super::format_number(known_input_tokens),
            super::format_number(context.context_budget_tokens)
        ),
        format!(
            "- Branch: {}",
            session_context.branch.unwrap_or("unavailable")
        ),
        format!(
            "- Git status: {}",
            if workspace_status_entries.is_empty() {
                "clean working tree"
            } else {
                "modified files listed below"
            }
        ),
        String::new(),
        "##### Completed".to_string(),
        String::new(),
    ];
    push_checkpoint_section(&mut lines, completed);
    lines.extend([String::new(), "##### Remaining".to_string(), String::new()]);
    push_checkpoint_section(&mut lines, remaining);
    lines.extend([String::new(), "##### Validation".to_string(), String::new()]);
    push_checkpoint_section(&mut lines, validation);
    lines.extend([
        String::new(),
        "##### Modified Files".to_string(),
        String::new(),
    ]);
    if workspace_status_entries.is_empty() {
        lines.push("- none".to_string());
    } else {
        for entry in workspace_status_entries {
            lines.push(format!("- {entry}"));
        }
    }
    lines.join("\n")
}

fn push_checkpoint_section(lines: &mut Vec<String>, section_lines: Vec<String>) {
    if section_lines.is_empty() {
        lines.push("- unavailable".to_string());
    } else {
        lines.extend(section_lines);
    }
}

fn update_resume_handle_after_turn(
    session_context: &mut WorkerSessionContext<'_>,
    turn_result: &TurnExecutionResult,
    turn_plan: ExecutionTurnPlan,
    issue_identifier: &str,
    turn_number: u32,
) {
    session_context.latest_resume_handle = turn_result
        .latest_resume_handle
        .clone()
        .or(session_context.latest_resume_handle.clone());
    if turn_plan.checkpoint_turn && turn_result.timeout.is_none() {
        session_context.latest_resume_handle = None;
    }
    if session_context.latest_resume_handle.is_some() {
        eprintln!("listen: captured resume handle for {issue_identifier} on turn {turn_number}",);
    } else if turn_plan.checkpoint_turn {
        eprintln!(
            "listen: cleared resume handle after checkpoint turn for {issue_identifier} on turn {turn_number}",
        );
    } else {
        eprintln!("listen: no resume handle captured for {issue_identifier} on turn {turn_number}",);
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ReviewReport {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    complete: bool,
    #[serde(default)]
    completed_items: Vec<String>,
    #[serde(default)]
    remaining_items: Vec<String>,
    #[serde(default)]
    validation_completed: Vec<String>,
    #[serde(default)]
    validation_remaining: Vec<String>,
    #[serde(default)]
    risks: Vec<String>,
    #[serde(default)]
    notes: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct FinalReviewReport {
    #[serde(default)]
    approved: bool,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    missing_items: Vec<String>,
    #[serde(default)]
    validation_gaps: Vec<String>,
    #[serde(default)]
    risks: Vec<String>,
    #[serde(default)]
    notes: Vec<String>,
}

enum ValidationGateOutcome {
    Passed(ReviewReport),
    Retry(ReviewReport),
    Exhausted(ReviewReport),
}

struct PrePrValidationGateContext<'a> {
    issue: &'a IssueSummary,
    turn_context: &'a ListenTurnContext<'a>,
    phase_context: WorkerPhaseContext<'a>,
    turns_completed: u32,
    pr_mutation_description: &'a str,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct VerificationAgentCriterion {
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: VerificationStatus,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    findings: Vec<VerificationFinding>,
    #[serde(default)]
    remediation: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct VerificationAgentBattleTest {
    #[serde(default)]
    input_path: String,
    #[serde(default)]
    status: VerificationStatus,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    remediation: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct VerificationAgentOutput {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    criteria: Vec<VerificationAgentCriterion>,
    #[serde(default)]
    battle_tests: Vec<VerificationAgentBattleTest>,
    #[serde(default)]
    notes: Vec<String>,
}

impl From<PullRequestLifecycleResult> for PullRequestSummary {
    fn from(value: PullRequestLifecycleResult) -> Self {
        Self {
            number: Some(value.number),
            url: Some(value.url),
            status: if value.is_draft {
                PullRequestStatus::Draft
            } else {
                PullRequestStatus::Ready
            },
        }
    }
}

async fn load_worker_issue<C>(service: &LinearService<C>, identifier: &str) -> Result<IssueSummary>
where
    C: LinearClient,
{
    let filters = IssueListFilters {
        team: issue_team_key(identifier),
        limit: 250,
        ..IssueListFilters::default()
    };

    for attempt in 0..3 {
        match service
            .find_issue_by_identifier(identifier, filters.clone())
            .await
        {
            Ok(Some(issue)) => return Ok(issue),
            Ok(None) => return Err(anyhow!("issue `{identifier}` was not found in Linear")),
            Err(error) if attempt < 2 && is_transient_linear_read_failure(&error) => {
                sleep(Duration::from_millis(100)).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(anyhow!("issue `{identifier}` was not found in Linear"))
}

fn is_transient_linear_read_failure(error: &anyhow::Error) -> bool {
    classify_linear_failure(error).is_retryable()
}

fn load_worker_backlog_issue(
    workspace_path: &Path,
    identifier: &str,
    parent_issue: &IssueSummary,
) -> Result<IssueSummary> {
    let issue_dir = PlanningPaths::new(workspace_path).backlog_issue_dir(identifier);
    let metadata = load_issue_metadata(&issue_dir).ok();
    Ok(IssueSummary {
        id: metadata
            .as_ref()
            .map(|metadata| metadata.issue_id.clone())
            .unwrap_or_else(|| identifier.to_string()),
        identifier: identifier.to_string(),
        title: metadata
            .as_ref()
            .map(|metadata| metadata.title.clone())
            .unwrap_or_else(|| parent_issue.title.clone()),
        description: None,
        url: metadata
            .as_ref()
            .map(|metadata| metadata.url.clone())
            .unwrap_or_default(),
        priority: parent_issue.priority,
        estimate: None,
        updated_at: parent_issue.updated_at.clone(),
        team: parent_issue.team.clone(),
        project: parent_issue.project.clone(),
        assignee: None,
        labels: Vec::new(),
        comments: Vec::new(),
        state: Some(WorkflowState {
            id: String::new(),
            name: BACKLOG_STATE.to_string(),
            kind: Some("backlog".to_string()),
        }),
        attachments: Vec::new(),
        parent: None,
        children: Vec::new(),
    })
}

fn stored_issue_snapshot_from_session(session: super::AgentSession) -> IssueSummary {
    let updated_at = DateTime::<Utc>::from_timestamp(session.updated_at_epoch_seconds as i64, 0)
        .map(|timestamp| timestamp.to_rfc3339())
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());
    let state = if session.phase.is_completed() {
        WorkflowState {
            id: "state-review".to_string(),
            name: "Human Review".to_string(),
            kind: Some("started".to_string()),
        }
    } else {
        WorkflowState {
            id: "state-in-progress".to_string(),
            name: "In Progress".to_string(),
            kind: Some("started".to_string()),
        }
    };
    let issue_identifier = session.issue_identifier;
    let team_key = session.team_key;
    IssueSummary {
        id: session.issue_id.unwrap_or_else(|| issue_identifier.clone()),
        identifier: issue_identifier.clone(),
        title: if session.issue_title.trim().is_empty() {
            format!("Stored session for {issue_identifier}")
        } else {
            session.issue_title
        },
        description: None,
        url: session.issue_url,
        priority: None,
        estimate: None,
        updated_at,
        team: crate::linear::TeamRef {
            id: format!("team-{}", team_key.to_ascii_lowercase()),
            key: team_key.clone(),
            name: team_key,
        },
        project: session.project_name.map(|name| crate::linear::ProjectRef {
            id: format!("project-{}", issue_identifier.to_ascii_lowercase()),
            name,
        }),
        assignee: None,
        labels: Vec::new(),
        comments: Vec::new(),
        state: Some(state),
        attachments: Vec::new(),
        parent: None,
        children: Vec::new(),
    }
}

fn load_existing_issue_snapshot(
    root: &Path,
    project_selector: Option<&str>,
    issue_identifier: &str,
) -> Result<Option<IssueSummary>> {
    let store = super::store::ListenProjectStore::resolve(root, project_selector)?;
    let state = store.load_state()?;
    Ok(state
        .sessions
        .into_iter()
        .find(|session| session.issue_matches(issue_identifier))
        .map(stored_issue_snapshot_from_session))
}

enum PendingLinearSyncReplayOutcome {
    Pending,
    Cleared,
    Completed,
}

async fn replay_pending_linear_sync(
    service: &LinearService<ReqwestLinearClient>,
    issue: &mut IssueSummary,
    context: &ListenTurnContext<'_>,
    session_context: &mut WorkerSessionContext<'_>,
    turns_completed: u32,
    provider_session_id: Option<&str>,
    log_path: &Path,
) -> Result<PendingLinearSyncReplayOutcome> {
    let Some(mut pending) = session_context.pending_linear_sync.clone() else {
        return Ok(PendingLinearSyncReplayOutcome::Cleared);
    };

    if pending.require_issue_refresh
        || pending.workpad_body.is_some()
        || pending.pull_request_attachment.is_some()
        || pending.review_transition_issue
    {
        match load_worker_issue(service, &issue.identifier).await {
            Ok(refreshed_issue) => {
                *issue = refreshed_issue;
                pending.require_issue_refresh = false;
            }
            Err(error) => {
                session_context.pending_linear_sync = Some(pending);
                record_pending_linear_sync_failure(
                    context.app_config,
                    &mut session_context.pending_linear_sync,
                    &error,
                );
                write_listen_session(
                    context.source_root,
                    context.project_selector,
                    build_worker_session(
                        issue,
                        SessionPhase::Publishing,
                        session_context
                            .pending_linear_sync
                            .as_ref()
                            .map(pending_linear_sync_summary)
                            .unwrap_or_else(|| "Pending Linear sync".to_string()),
                        session_context,
                        turns_completed,
                        provider_session_id,
                        &session_context.canonical,
                    ),
                )?;
                append_worker_log(
                    log_path,
                    "pending linear sync",
                    &[format!("Pending replay failed to refresh issue: {error:#}")],
                )?;
                return Ok(PendingLinearSyncReplayOutcome::Pending);
            }
        }
    }

    if let Some(body) = pending.workpad_body.clone() {
        match service
            .update_workpad_comment_by_id(context.workpad_comment_id, body.clone())
            .await
        {
            Ok(_) => pending.workpad_body = None,
            Err(error) => {
                session_context.pending_linear_sync = Some(pending);
                record_pending_linear_sync_failure(
                    context.app_config,
                    &mut session_context.pending_linear_sync,
                    &error,
                );
                return Ok(PendingLinearSyncReplayOutcome::Pending);
            }
        }
    }

    if let Some(attachment) = pending.pull_request_attachment.clone() {
        if !issue
            .attachments
            .iter()
            .any(|existing| existing.url == attachment.url)
        {
            match service
                .create_attachment(AttachmentCreateRequest {
                    issue_id: issue.id.clone(),
                    title: format!("GitHub PR #{}", attachment.number),
                    url: attachment.url.clone(),
                    metadata: json!({
                        "provider": "github",
                        "type": "pull_request"
                    }),
                })
                .await
            {
                Ok(_) => pending.pull_request_attachment = None,
                Err(error) => {
                    session_context.pending_linear_sync = Some(pending);
                    record_pending_linear_sync_failure(
                        context.app_config,
                        &mut session_context.pending_linear_sync,
                        &error,
                    );
                    return Ok(PendingLinearSyncReplayOutcome::Pending);
                }
            }
        } else {
            pending.pull_request_attachment = None;
        }
    }

    if pending.review_transition_issue {
        match try_transition_issue_to_review_state(service, issue).await {
            Ok(transitioned_issue) => {
                if let Some(updated_issue) = transitioned_issue {
                    *issue = updated_issue;
                }
                pending.review_transition_issue = false;
            }
            Err(error) => {
                session_context.pending_linear_sync = Some(pending);
                record_pending_linear_sync_failure(
                    context.app_config,
                    &mut session_context.pending_linear_sync,
                    &error,
                );
                return Ok(PendingLinearSyncReplayOutcome::Pending);
            }
        }
    }

    if pending.review_transition_backlog_issue.is_some()
        && let Some(backlog_issue) = context.backlog_issue
    {
        match try_transition_issue_to_review_state(service, backlog_issue).await {
            Ok(_) => pending.review_transition_backlog_issue = None,
            Err(error) => {
                session_context.pending_linear_sync = Some(pending);
                record_pending_linear_sync_failure(
                    context.app_config,
                    &mut session_context.pending_linear_sync,
                    &error,
                );
                return Ok(PendingLinearSyncReplayOutcome::Pending);
            }
        }
    }

    session_context.pending_linear_sync = Some(pending);
    clear_pending_linear_sync_failure(&mut session_context.pending_linear_sync);
    if session_context
        .pending_linear_sync
        .as_ref()
        .is_some_and(PendingLinearSync::is_empty)
    {
        session_context.pending_linear_sync = None;
    }

    if !listen_issue_is_active(issue.state.as_ref().map(|state| state.name.as_str())) {
        write_listen_session(
            context.source_root,
            context.project_selector,
            build_worker_session(
                issue,
                SessionPhase::Completed,
                compact_completed_summary(
                    issue.description.as_deref(),
                    turns_completed,
                    &issue_state_label(issue),
                ),
                session_context,
                turns_completed,
                provider_session_id,
                &session_context.canonical,
            ),
        )?;
        try_listener_auto_clean(
            context.source_root,
            context.project_selector,
            context.workspace_path,
            &issue.identifier,
        );
        return Ok(PendingLinearSyncReplayOutcome::Completed);
    }

    if session_context.pending_linear_sync.is_some() {
        Ok(PendingLinearSyncReplayOutcome::Pending)
    } else {
        Ok(PendingLinearSyncReplayOutcome::Cleared)
    }
}

fn listener_pull_request_title(issue: &IssueSummary) -> String {
    format!("{}: {}", issue.identifier, issue.title)
}

fn listener_linear_identifier_pr_label(issue: &IssueSummary) -> String {
    format!("id-{}", issue.identifier)
}

fn listener_pull_request_body(
    issue: &IssueSummary,
    review: Option<&ReviewReport>,
    verification: Option<&VerificationSummary>,
) -> String {
    let mut lines = vec![
        format!("# {}", listener_pull_request_title(issue)),
        String::new(),
        "## Summary".to_string(),
        format!("- Linear issue: {}", issue.url),
        format!(
            "- Published automatically by `{} agents listen` for `{}`",
            crate::branding::COMMAND_NAME,
            issue.identifier,
        ),
    ];

    if let Some(review) = review {
        lines.push(format!("- Latest listener review: {}", review.summary));
    }
    if let Some(verification) = verification {
        lines.push(format!(
            "- Latest verification: {} ({})",
            verification.summary,
            verification.compact_label()
        ));
    }

    lines.extend([
        String::new(),
        "## Lifecycle".to_string(),
        "- Initial publication uses a draft PR for unattended work in progress.".to_string(),
        "- The same PR is promoted to ready for review during the existing review handoff."
            .to_string(),
    ]);

    if let Some(review) = review {
        if !review.completed_items.is_empty() {
            lines.extend([String::new(), "## Completed In This Branch".to_string()]);
            for item in &review.completed_items {
                lines.push(format!("- {item}"));
            }
        }

        if !review.remaining_items.is_empty() {
            lines.extend([String::new(), "## Remaining Work".to_string()]);
            for item in &review.remaining_items {
                lines.push(format!("- {item}"));
            }
        }

        if !review.validation_completed.is_empty() || !review.validation_remaining.is_empty() {
            lines.extend([String::new(), "## Validation".to_string()]);
            for item in &review.validation_completed {
                lines.push(format!("- Completed: {item}"));
            }
            for item in &review.validation_remaining {
                lines.push(format!("- Remaining: {item}"));
            }
        }
    }

    if let Some(verification) = verification {
        lines.extend([String::new(), "## Verification".to_string()]);
        lines.push(format!("- Status: {}", verification.status.display_label()));
        lines.push(format!("- Summary: {}", verification.summary));
        if verification.criteria_total > 0 {
            lines.push(format!(
                "- Criteria failures: {}/{}",
                verification.criteria_failed, verification.criteria_total
            ));
        }
        lines.push(format!(
            "- E2E: {}",
            verification.e2e_status.display_label()
        ));
        lines.push(format!(
            "- Battle tests: {}",
            verification.battle_test_status.display_label()
        ));
        for item in &verification.remediation {
            lines.push(format!("- Remediation: {item}"));
        }
    }

    if let Some(description) = issue.description.as_deref()
        && !description.trim().is_empty()
    {
        lines.push(String::new());
        lines.push("## Issue Context".to_string());
        lines.push(description.trim().to_string());
    }

    lines.join("\n")
}

fn update_pending_linear_sync<F>(pending_linear_sync: &mut Option<PendingLinearSync>, updater: F)
where
    F: FnOnce(&mut PendingLinearSync),
{
    let mut pending = pending_linear_sync.take().unwrap_or_default();
    updater(&mut pending);
    *pending_linear_sync = (!pending.is_empty()).then_some(pending);
}

fn record_pending_linear_sync_failure(
    app_config: &AppConfig,
    pending_linear_sync: &mut Option<PendingLinearSync>,
    error: &anyhow::Error,
) {
    let classified = classify_linear_failure(error);
    update_pending_linear_sync(pending_linear_sync, |pending| {
        let consecutive_failures = pending
            .last_failure
            .as_ref()
            .map(|failure| failure.consecutive_failures)
            .unwrap_or(0)
            .saturating_add(1);
        let observed_at_epoch_seconds = now_epoch_seconds();
        pending.last_failure = Some(super::LinearFailureSnapshot {
            kind: classified.kind,
            message: classified.message.clone(),
            observed_at_epoch_seconds,
            status_code: classified.status_code,
            consecutive_failures,
            next_retry_at_epoch_seconds: classified.is_retryable().then_some(
                observed_at_epoch_seconds
                    + app_config
                        .defaults
                        .listen
                        .retry
                        .backoff_seconds_for_failure_streak(consecutive_failures),
            ),
        });
    });
}

fn clear_pending_linear_sync_failure(pending_linear_sync: &mut Option<PendingLinearSync>) {
    if let Some(pending) = pending_linear_sync.as_mut() {
        pending.last_failure = None;
    }
    if pending_linear_sync
        .as_ref()
        .is_some_and(PendingLinearSync::is_empty)
    {
        *pending_linear_sync = None;
    }
}

fn pending_linear_sync_summary(pending_linear_sync: &PendingLinearSync) -> String {
    let operations = pending_linear_sync.operation_labels().join(", ");
    let failure = pending_linear_sync
        .last_failure
        .as_ref()
        .map(|failure| {
            format!(
                "{} failure | retry {}",
                failure.kind.label(),
                failure.retry_label(now_epoch_seconds())
            )
        })
        .unwrap_or_else(|| "waiting to replay".to_string());
    compact_session_summary([
        Some(format!("Pending Linear sync: {operations}")),
        Some(failure),
    ])
}

fn defer_pending_linear_sync_operation<F>(
    app_config: &AppConfig,
    pending_linear_sync: &mut Option<PendingLinearSync>,
    error: &anyhow::Error,
    operation_label: &str,
    log_path: &Path,
    updater: F,
) -> Result<()>
where
    F: FnOnce(&mut PendingLinearSync),
{
    update_pending_linear_sync(pending_linear_sync, updater);
    record_pending_linear_sync_failure(app_config, pending_linear_sync, error);
    append_worker_log(
        log_path,
        "pending linear sync",
        &[format!("Deferred {operation_label}: {error:#}")],
    )
}

fn write_pending_linear_sync_blocked_session(
    issue: &IssueSummary,
    session_context: &WorkerSessionContext<'_>,
    turns_completed: u32,
    provider_session_id: Option<&str>,
    log_path: &Path,
) -> Result<()> {
    let blocked = blocked_reason(BlockedCategory::Infra, "pending Linear sync", true);
    let summary = compact_session_summary([
        Some(blocked.summary_headline()),
        session_context
            .pending_linear_sync
            .as_ref()
            .map(pending_linear_sync_summary),
        Some(format!("see {}", log_path.display())),
    ]);
    write_listen_session(
        session_context.source_root,
        session_context.project_selector,
        build_worker_blocked_session(
            issue,
            blocked.clone(),
            summary,
            session_context,
            turns_completed,
            provider_session_id,
            &session_context.canonical,
        ),
    )
}

fn write_listener_pull_request_body(
    workspace_path: &Path,
    issue: &IssueSummary,
    review: Option<&ReviewReport>,
    verification: Option<&VerificationSummary>,
) -> Result<std::path::PathBuf> {
    let path = PlanningPaths::new(workspace_path)
        .agent_dir
        .join(format!("{}-pull-request.md", issue.identifier));
    write_text_file(
        &path,
        &listener_pull_request_body(issue, review, verification),
        true,
    )?;
    Ok(path)
}

fn ensure_listener_pull_request_label(
    gh: &GhCli,
    workspace_path: &Path,
    issue: &IssueSummary,
    pull_request: &PullRequestLifecycleResult,
) -> Result<()> {
    gh.ensure_label_exists(
        workspace_path,
        REQUIRED_LISTEN_PR_LABEL,
        REQUIRED_LISTEN_PR_LABEL_COLOR,
        REQUIRED_LISTEN_PR_LABEL_DESCRIPTION,
    )?;
    gh.add_label_to_pull_request(
        workspace_path,
        pull_request.number,
        REQUIRED_LISTEN_PR_LABEL,
    )?;

    let linear_identifier_label = listener_linear_identifier_pr_label(issue);
    gh.ensure_label_exists(
        workspace_path,
        &linear_identifier_label,
        LINEAR_IDENTIFIER_PR_LABEL_COLOR,
        &format!("Linear issue {}", issue.identifier),
    )?;
    gh.add_label_to_pull_request(
        workspace_path,
        pull_request.number,
        &linear_identifier_label,
    )
}

async fn ensure_listener_pull_request_attachment<C>(
    service: &LinearService<C>,
    issue: &IssueSummary,
    pull_request: &PullRequestLifecycleResult,
) -> Result<()>
where
    C: LinearClient,
{
    if issue
        .attachments
        .iter()
        .any(|attachment| attachment.url == pull_request.url)
    {
        return Ok(());
    }

    service
        .create_attachment(AttachmentCreateRequest {
            issue_id: issue.id.clone(),
            title: format!("GitHub PR #{}", pull_request.number),
            url: pull_request.url.clone(),
            metadata: json!({
                "provider": "github",
                "type": "pull_request"
            }),
        })
        .await?;
    Ok(())
}

async fn publish_listener_pull_request(
    issue: &IssueSummary,
    workspace_path: &Path,
    branch: &str,
    mode: PullRequestPublishMode,
    review: Option<&ReviewReport>,
    verification: Option<&VerificationSummary>,
) -> Result<Option<PullRequestLifecycleResult>> {
    if branch.eq_ignore_ascii_case(LISTEN_PULL_REQUEST_BASE_BRANCH) {
        return Ok(None);
    }

    let gh = GhCli;
    let body_path = write_listener_pull_request_body(workspace_path, issue, review, verification)?;
    let title = listener_pull_request_title(issue);
    let pull_request = gh.publish_branch_pull_request(
        workspace_path,
        PullRequestPublishRequest {
            head_branch: branch,
            base_branch: LISTEN_PULL_REQUEST_BASE_BRANCH,
            title: &title,
            body_path: &body_path,
            mode,
        },
    )?;
    ensure_listener_pull_request_label(&gh, workspace_path, issue, &pull_request)?;
    Ok(Some(pull_request))
}

async fn prepare_listener_pull_request_for_review(
    issue: &IssueSummary,
    workspace_path: &Path,
    branch: &str,
    existing_pull_request: &PullRequestSummary,
    review: &ReviewReport,
    verification: Option<&VerificationSummary>,
) -> Result<Option<PullRequestLifecycleResult>> {
    if branch.eq_ignore_ascii_case(LISTEN_PULL_REQUEST_BASE_BRANCH) {
        return Ok(None);
    }

    let gh = GhCli;
    let body_path =
        write_listener_pull_request_body(workspace_path, issue, Some(review), verification)?;
    let title = listener_pull_request_title(issue);
    let pull_request = if let Some(number) = existing_pull_request.number {
        gh.refresh_pull_request_by_number(workspace_path, number, &title, &body_path)?;
        gh.promote_pull_request_to_ready(workspace_path, number)?
    } else if let Some(existing) = gh.refresh_existing_branch_pull_request(
        workspace_path,
        PullRequestPublishRequest {
            head_branch: branch,
            base_branch: LISTEN_PULL_REQUEST_BASE_BRANCH,
            title: &title,
            body_path: &body_path,
            mode: PullRequestPublishMode::Draft,
        },
    )? {
        gh.promote_pull_request_to_ready(workspace_path, existing.number)?
    } else {
        gh.publish_branch_pull_request(
            workspace_path,
            PullRequestPublishRequest {
                head_branch: branch,
                base_branch: LISTEN_PULL_REQUEST_BASE_BRANCH,
                title: &title,
                body_path: &body_path,
                mode: PullRequestPublishMode::Ready,
            },
        )?
    };
    if pull_request.is_draft {
        bail!(
            "pull request #{} for `{}` is still draft after review handoff",
            pull_request.number,
            issue.identifier
        );
    }
    ensure_listener_pull_request_label(&gh, workspace_path, issue, &pull_request)?;
    Ok(Some(pull_request))
}

fn load_listen_validation_profile(workspace_path: &Path) -> Result<ResolvedValidationProfile> {
    let planning_meta = PlanningMeta::load(workspace_path)
        .with_context(|| format!("failed to load `{}`", workspace_path.display()))?;
    resolve_validation_profile(workspace_path, &planning_meta, &[])
}

fn append_worker_log(log_path: &Path, section: &str, lines: &[String]) -> Result<()> {
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .with_context(|| format!("failed to open `{}`", log_path.display()))?;
    writeln!(log, "\n--- listen {section} @ {} ---", now_timestamp())
        .with_context(|| format!("failed to write `{}`", log_path.display()))?;
    for line in lines {
        writeln!(log, "{line}")
            .with_context(|| format!("failed to write `{}`", log_path.display()))?;
    }
    Ok(())
}

fn render_validation_result_lines(records: &[ValidationCommandRecord]) -> Vec<String> {
    let mut lines = Vec::new();
    for record in records {
        lines.push(format!(
            "command={} exit_code={}",
            record.command, record.exit_code
        ));
        if let Some(excerpt) = output_excerpt(&record.stderr) {
            lines.push(format!("stderr: {excerpt}"));
        }
        if let Some(excerpt) = output_excerpt(&record.stdout) {
            lines.push(format!("stdout: {excerpt}"));
        }
    }
    lines
}

fn output_excerpt(text: &str) -> Option<String> {
    let excerpt = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" | ");
    if excerpt.is_empty() {
        None
    } else if excerpt.chars().count() > 240 {
        Some(truncate_with_ellipsis(&excerpt, 240))
    } else {
        Some(excerpt)
    }
}

fn push_unique(values: &mut Vec<String>, item: impl Into<String>) {
    let item = item.into();
    if !values.iter().any(|existing| existing == &item) {
        values.push(item);
    }
}

fn review_with_validation_success(
    review: &ReviewReport,
    profile: &ResolvedValidationProfile,
) -> ReviewReport {
    let mut updated = review.clone();
    push_unique(
        &mut updated.validation_completed,
        format!(
            "Local validation profile `{}` passed: {}",
            profile
                .profile_label
                .as_deref()
                .unwrap_or(profile.source.label()),
            profile.commands.join(" && ")
        ),
    );
    updated
}

fn review_for_validation_failure(
    review: &ReviewReport,
    profile: &ResolvedValidationProfile,
    records: &[ValidationCommandRecord],
    remaining_repair_turns: usize,
    pr_mutation_description: &str,
) -> ReviewReport {
    let mut follow_up = review.clone();
    follow_up.complete = false;
    follow_up.summary = format!(
        "Validation failed before {pr_mutation_description}; repair is required before PR mutation."
    );
    push_unique(
        &mut follow_up.remaining_items,
        format!(
            "Repair the local validation failure and rerun the validation gate before {pr_mutation_description}."
        ),
    );
    push_unique(
        &mut follow_up.validation_remaining,
        format!(
            "Local validation profile `{}` must pass: {}",
            profile
                .profile_label
                .as_deref()
                .unwrap_or(profile.source.label()),
            profile.commands.join(" && ")
        ),
    );
    for record in records.iter().filter(|record| record.exit_code != 0) {
        push_unique(
            &mut follow_up.risks,
            format!(
                "Validation command `{}` failed with exit code {}.",
                record.command, record.exit_code
            ),
        );
        if let Some(excerpt) =
            output_excerpt(&record.stderr).or_else(|| output_excerpt(&record.stdout))
        {
            push_unique(
                &mut follow_up.notes,
                format!("Validation failure detail: {excerpt}"),
            );
        }
    }
    push_unique(
        &mut follow_up.notes,
        format!("Validation repair turns remaining after this retry: {remaining_repair_turns}"),
    );
    follow_up
}

fn review_for_verification_failure(
    review: &ReviewReport,
    verification: &VerificationReport,
    remaining_repair_turns: usize,
) -> ReviewReport {
    let mut follow_up = review.clone();
    follow_up.complete = false;
    follow_up.summary =
        "Verification failed before ready promotion; repair is required before PR mutation."
            .to_string();
    push_unique(
        &mut follow_up.remaining_items,
        "Repair the verification findings and rerun the verification gate before ready promotion."
            .to_string(),
    );
    push_unique(
        &mut follow_up.validation_remaining,
        "Verification must pass before validation and ready promotion can succeed.".to_string(),
    );
    push_unique(
        &mut follow_up.notes,
        format!("Verification summary: {}", verification.summary),
    );
    if verification.code_review.status == VerificationStatus::Failed {
        push_unique(
            &mut follow_up.risks,
            verification.code_review.summary.clone(),
        );
    }
    if verification.e2e.status == VerificationStatus::Failed {
        push_unique(&mut follow_up.risks, verification.e2e.summary.clone());
    }
    if verification.battle_tests.status == VerificationStatus::Failed {
        push_unique(
            &mut follow_up.risks,
            verification.battle_tests.summary.clone(),
        );
    }
    for item in &verification.remediation {
        push_unique(&mut follow_up.remaining_items, item.clone());
    }
    push_unique(
        &mut follow_up.notes,
        format!("Verification repair turns remaining after this retry: {remaining_repair_turns}"),
    );
    follow_up
}

fn review_for_turn_timeout(timeout: &SessionTimeoutRecord) -> ReviewReport {
    ReviewReport {
        summary: format!(
            "Execution timed out on turn {} after {}s (limit {}s); subprocess pid {} was terminated with {}.",
            timeout.turn,
            timeout.elapsed_seconds,
            timeout.timeout_seconds,
            timeout.pid,
            timeout.termination.label()
        ),
        complete: false,
        remaining_items: vec![
            "Repair the timeout root cause from the previous listen turn before review handoff."
                .to_string(),
        ],
        risks: vec![format!(
            "Turn {} timed out and required {} for pid {}.",
            timeout.turn,
            timeout.termination.label(),
            timeout.pid
        )],
        notes: vec![format!(
            "Timeout remains distinct from stalled-turn handling. Grace window: {}s.",
            timeout.graceful_shutdown_seconds
        )],
        ..ReviewReport::default()
    }
}

fn phase_timeout_error(phase: &str, timeout: &SessionTimeoutRecord) -> anyhow::Error {
    anyhow!(
        "listen {phase} timed out on turn {} after {}s (limit {}s); subprocess pid {} was terminated with {}",
        timeout.turn,
        timeout.elapsed_seconds,
        timeout.timeout_seconds,
        timeout.pid,
        timeout.termination.label()
    )
}

async fn run_pre_pr_validation_gate(
    gate_context: PrePrValidationGateContext<'_>,
    review: &ReviewReport,
    remaining_validation_repair_turns: &mut usize,
) -> Result<ValidationGateOutcome> {
    let validation_profile =
        load_listen_validation_profile(gate_context.turn_context.workspace_path)?;
    write_listen_session(
        gate_context.phase_context.source_root,
        gate_context.phase_context.project_selector,
        build_worker_session(
            gate_context.issue,
            SessionPhase::Validating,
            compact_session_summary([
                Some(format!(
                    "Validating before {} with {}",
                    gate_context.pr_mutation_description,
                    validation_profile.source.label()
                )),
                Some(format!(
                    "see {}",
                    gate_context.phase_context.log_path.display()
                )),
            ]),
            gate_context.phase_context.session_context,
            gate_context.turns_completed,
            gate_context.phase_context.provider_session_id,
            &gate_context.phase_context.session_context.canonical,
        ),
    )?;
    append_worker_log(
        gate_context.phase_context.log_path,
        "validation profile",
        &validation_profile.diagnostics_lines(),
    )?;
    let validation_records = run_validation_commands(
        gate_context.turn_context.workspace_path,
        &validation_profile.commands,
    )?;
    append_worker_log(
        gate_context.phase_context.log_path,
        "validation results",
        &render_validation_result_lines(&validation_records),
    )?;
    if validation_records
        .iter()
        .all(|record| record.exit_code == 0)
    {
        return Ok(ValidationGateOutcome::Passed(
            review_with_validation_success(review, &validation_profile),
        ));
    }

    let budget_exhausted = *remaining_validation_repair_turns == 0;
    let remaining_after_failure = remaining_validation_repair_turns.saturating_sub(1);
    let follow_up_review = review_for_validation_failure(
        review,
        &validation_profile,
        &validation_records,
        remaining_after_failure,
        gate_context.pr_mutation_description,
    );
    if budget_exhausted {
        let blocked = blocked_reason(
            BlockedCategory::Gate,
            "validation failed and repair budget exhausted",
            false,
        );
        write_listen_session(
            gate_context.phase_context.source_root,
            gate_context.phase_context.project_selector,
            build_worker_blocked_session(
                gate_context.issue,
                blocked.clone(),
                compact_blocked_summary(
                    &blocked,
                    gate_context.issue.description.as_deref(),
                    gate_context.phase_context.log_path,
                ),
                gate_context.phase_context.session_context,
                gate_context.turns_completed,
                gate_context.phase_context.provider_session_id,
                &gate_context.phase_context.session_context.canonical,
            ),
        )?;
        return Ok(ValidationGateOutcome::Exhausted(follow_up_review));
    }

    *remaining_validation_repair_turns -= 1;
    Ok(ValidationGateOutcome::Retry(follow_up_review))
}

fn review_for_ci_failure(
    previous_review: Option<&ReviewReport>,
    pull_request_number: u64,
    checks: &[PullRequestCheck],
    remaining_repair_turns: usize,
) -> ReviewReport {
    let mut follow_up = previous_review.cloned().unwrap_or_default();
    follow_up.complete = false;
    follow_up.summary = format!(
        "GitHub CI failed for PR #{pull_request_number}; repair the existing branch PR and rerun local validation."
    );
    push_unique(
        &mut follow_up.remaining_items,
        format!(
            "Repair failing GitHub checks on PR #{pull_request_number} and update the same PR."
        ),
    );
    push_unique(
        &mut follow_up.validation_remaining,
        format!(
            "Post-publication CI must pass for PR #{pull_request_number} before review handoff."
        ),
    );
    for check in checks {
        push_unique(
            &mut follow_up.risks,
            format!("Failing check `{}` is still red.", check.name),
        );
        let mut detail = format!("Check `{}` reported `{}`.", check.name, check.state);
        if let Some(description) = check
            .description
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            detail.push_str(&format!(" {description}"));
        }
        if let Some(link) = check.link.as_deref().filter(|value| !value.is_empty()) {
            detail.push_str(&format!(" ({link})"));
        }
        push_unique(&mut follow_up.notes, detail);
    }
    push_unique(
        &mut follow_up.notes,
        format!("Validation repair turns remaining after this retry: {remaining_repair_turns}"),
    );
    follow_up
}

fn render_check_failure_lines(number: u64, checks: &[PullRequestCheck]) -> Vec<String> {
    let mut lines = vec![format!(
        "pull request #{number} has {} failing check(s)",
        checks.len()
    )];
    for check in checks {
        let mut line = format!(
            "check={} bucket={} state={}",
            check.name, check.bucket, check.state
        );
        if let Some(link) = check.link.as_deref().filter(|value| !value.is_empty()) {
            line.push_str(&format!(" link={link}"));
        }
        lines.push(line);
    }
    lines
}

enum PostPublicationCiGateOutcome {
    Passed,
    Retry(ReviewReport),
    Blocked,
}

struct PostPublicationCiGateContext<'a, 'ctx> {
    issue: &'a IssueSummary,
    workspace_path: &'a Path,
    review: &'a ReviewReport,
    summary_prefix: &'a str,
    turn_context: &'a ListenTurnContext<'ctx>,
    turns_completed: u32,
    provider_session_id: Option<&'a str>,
    log_path: &'a Path,
    service: &'a LinearService<ReqwestLinearClient>,
}

async fn run_post_publication_ci_settle_gate(
    pull_request_number: u64,
    gate_context: &PostPublicationCiGateContext<'_, '_>,
    session_context: &mut WorkerSessionContext<'_>,
    remaining_validation_repair_turns: &mut usize,
) -> Result<PostPublicationCiGateOutcome> {
    let ci_poll_interval = gate_context
        .turn_context
        .app_config
        .defaults
        .listen
        .ci_poll_interval_seconds();
    let ci_poll_timeout = gate_context
        .turn_context
        .app_config
        .defaults
        .listen
        .ci_poll_timeout_seconds();
    let ci_timeout_behavior = gate_context
        .turn_context
        .app_config
        .defaults
        .listen
        .ci_timeout_behavior();
    let mut elapsed_seconds = 0u64;

    loop {
        let snapshot =
            GhCli.pull_request_checks_snapshot(gate_context.workspace_path, pull_request_number)?;
        match snapshot.outcome {
            PullRequestChecksOutcome::NoChecksConfigured => {
                let ci_summary = "no GitHub checks configured".to_string();
                session_context.github_ci_summary = Some(ci_summary.clone());
                write_listen_session(
                    session_context.source_root,
                    session_context.project_selector,
                    build_worker_session(
                        gate_context.issue,
                        SessionPhase::Publishing,
                        compact_session_summary([
                            Some(gate_context.summary_prefix.to_string()),
                            Some(ci_summary.clone()),
                        ]),
                        session_context,
                        gate_context.turns_completed,
                        gate_context.provider_session_id,
                        &session_context.canonical,
                    ),
                )?;
                append_worker_log(
                    gate_context.log_path,
                    "pull request checks",
                    &[format!(
                        "pull request #{pull_request_number} has no GitHub checks configured"
                    )],
                )?;
                return Ok(PostPublicationCiGateOutcome::Passed);
            }
            PullRequestChecksOutcome::Passed => {
                let ci_summary = format!(
                    "GitHub CI passed {}/{}",
                    snapshot.settled_count, snapshot.total_count
                );
                session_context.github_ci_summary = Some(ci_summary.clone());
                write_listen_session(
                    session_context.source_root,
                    session_context.project_selector,
                    build_worker_session(
                        gate_context.issue,
                        SessionPhase::Publishing,
                        compact_session_summary([
                            Some(gate_context.summary_prefix.to_string()),
                            Some(ci_summary.clone()),
                        ]),
                        session_context,
                        gate_context.turns_completed,
                        gate_context.provider_session_id,
                        &session_context.canonical,
                    ),
                )?;
                append_worker_log(
                    gate_context.log_path,
                    "pull request checks",
                    &[format!(
                        "pull request #{pull_request_number} settled green ({}/{})",
                        snapshot.settled_count, snapshot.total_count
                    )],
                )?;
                return Ok(PostPublicationCiGateOutcome::Passed);
            }
            PullRequestChecksOutcome::Failed => {
                session_context.github_ci_summary = None;
                let failing_checks = snapshot.failed_checks();
                let budget_exhausted = *remaining_validation_repair_turns == 0;
                let remaining_after_failure =
                    (*remaining_validation_repair_turns).saturating_sub(1);
                let follow_up_review = review_for_ci_failure(
                    Some(gate_context.review),
                    pull_request_number,
                    &failing_checks,
                    remaining_after_failure,
                );
                sync_review_tracking(
                    gate_context.service,
                    gate_context.issue,
                    gate_context.turn_context,
                    gate_context.turn_context.app_config,
                    session_context,
                    gate_context.log_path,
                    &follow_up_review,
                )
                .await?;
                append_worker_log(
                    gate_context.log_path,
                    "pull request checks",
                    &render_check_failure_lines(pull_request_number, &failing_checks),
                )?;
                if budget_exhausted {
                    let blocked =
                        blocked_reason(BlockedCategory::Gate, "CI repair budget exhausted", false);
                    write_listen_session(
                        session_context.source_root,
                        session_context.project_selector,
                        build_worker_blocked_session(
                            gate_context.issue,
                            blocked.clone(),
                            compact_blocked_summary(
                                &blocked,
                                gate_context.issue.description.as_deref(),
                                gate_context.log_path,
                            ),
                            session_context,
                            gate_context.turns_completed,
                            gate_context.provider_session_id,
                            &session_context.canonical,
                        ),
                    )?;
                    return Ok(PostPublicationCiGateOutcome::Blocked);
                }
                *remaining_validation_repair_turns -= 1;
                return Ok(PostPublicationCiGateOutcome::Retry(follow_up_review));
            }
            PullRequestChecksOutcome::Pending => {
                if elapsed_seconds >= ci_poll_timeout {
                    let timeout_reason = format!(
                        "GitHub CI settle timed out after {}s ({}/{}) settled",
                        ci_poll_timeout, snapshot.settled_count, snapshot.total_count
                    );
                    append_worker_log(
                        gate_context.log_path,
                        "pull request checks",
                        &[format!(
                            "pull request #{pull_request_number} {timeout_reason}"
                        )],
                    )?;
                    match ci_timeout_behavior {
                        crate::config::ListenCiTimeoutBehavior::Block => {
                            session_context.github_ci_summary = None;
                            let blocked =
                                blocked_reason(BlockedCategory::Gate, timeout_reason, true);
                            write_listen_session(
                                session_context.source_root,
                                session_context.project_selector,
                                build_worker_blocked_session(
                                    gate_context.issue,
                                    blocked.clone(),
                                    compact_blocked_summary(
                                        &blocked,
                                        gate_context.issue.description.as_deref(),
                                        gate_context.log_path,
                                    ),
                                    session_context,
                                    gate_context.turns_completed,
                                    gate_context.provider_session_id,
                                    &session_context.canonical,
                                ),
                            )?;
                            return Ok(PostPublicationCiGateOutcome::Blocked);
                        }
                        crate::config::ListenCiTimeoutBehavior::WarnAndProceed => {
                            let ci_summary = format!(
                                "GitHub CI timeout warning after {}s ({}/{}) settled",
                                ci_poll_timeout, snapshot.settled_count, snapshot.total_count
                            );
                            session_context.github_ci_summary = Some(ci_summary.clone());
                            write_listen_session(
                                session_context.source_root,
                                session_context.project_selector,
                                build_worker_session(
                                    gate_context.issue,
                                    SessionPhase::Publishing,
                                    compact_session_summary([
                                        Some(gate_context.summary_prefix.to_string()),
                                        Some(ci_summary.clone()),
                                    ]),
                                    session_context,
                                    gate_context.turns_completed,
                                    gate_context.provider_session_id,
                                    &session_context.canonical,
                                ),
                            )?;
                            return Ok(PostPublicationCiGateOutcome::Passed);
                        }
                    }
                }

                let remaining_seconds = ci_poll_timeout.saturating_sub(elapsed_seconds);
                write_listen_session(
                    session_context.source_root,
                    session_context.project_selector,
                    build_worker_session(
                        gate_context.issue,
                        SessionPhase::Publishing,
                        compact_session_summary([
                            Some(gate_context.summary_prefix.to_string()),
                            Some(format!(
                                "waiting for GitHub CI {}/{} settled",
                                snapshot.settled_count, snapshot.total_count
                            )),
                            Some(format!("{remaining_seconds}s remaining")),
                        ]),
                        session_context,
                        gate_context.turns_completed,
                        gate_context.provider_session_id,
                        &session_context.canonical,
                    ),
                )?;
                append_worker_log(
                    gate_context.log_path,
                    "pull request checks",
                    &[format!(
                        "pull request #{pull_request_number} waiting for GitHub CI {}/{} settled ({}s remaining)",
                        snapshot.settled_count, snapshot.total_count, remaining_seconds
                    )],
                )?;

                let sleep_seconds =
                    ci_poll_interval.min(ci_poll_timeout.saturating_sub(elapsed_seconds));
                if sleep_seconds == 0 {
                    continue;
                }
                sleep(Duration::from_secs(sleep_seconds)).await;
                elapsed_seconds = elapsed_seconds.saturating_add(sleep_seconds);
            }
        }
    }
}

fn build_listen_run_args(
    issue: &IssueSummary,
    turn_number: u32,
    context: &ListenTurnContext<'_>,
    plan: ExecutionTurnPlan,
    previous_review: Option<&ReviewReport>,
    verification_summary: Option<&VerificationSummary>,
    has_resume_handle: bool,
) -> Result<RunAgentArgs> {
    let use_continuation = has_resume_handle && turn_number > 1;
    let effective_verification_summary = if turn_number > 1 {
        match verification_summary.cloned() {
            Some(summary) => Some(summary),
            None => load_existing_verification_summary(
                context.source_root,
                context.project_selector,
                &issue.identifier,
            )
            .ok()
            .flatten(),
        }
    } else {
        verification_summary.cloned()
    };

    let prompt = if turn_number > 1 {
        render_execution_delta_prompt(
            issue,
            turn_number,
            plan.max_turns,
            previous_review,
            effective_verification_summary.as_ref(),
            use_continuation,
            plan,
            &context_pressure_guidance(plan, context, use_continuation),
        )
    } else {
        let mut prompt = render_agent_prompt(
            issue,
            context.workspace_path,
            context.workpad_comment_id,
            context.backlog_issue,
            turn_number,
            plan.max_turns,
        );
        let guidance = context_pressure_guidance(plan, context, use_continuation);
        if !guidance.is_empty() {
            prompt.push_str("\n\nContext pressure guidance:\n");
            prompt.push_str(&guidance);
        }
        prompt
    };

    let instructions = if use_continuation {
        None
    } else {
        Some(build_agent_instructions(issue, turn_number, context, plan)?)
    };

    Ok(RunAgentArgs {
        root: Some(context.source_root.to_path_buf()),
        route_key: Some(AGENT_ROUTE_AGENTS_LISTEN.to_string()),
        agent: context.args.agent.clone(),
        prompt,
        instructions,
        model: context.args.model.clone(),
        reasoning: context.args.reasoning.clone(),
        transport: None,
        attachments: Vec::new(),
    })
}

fn build_review_run_args(
    issue: &IssueSummary,
    turn_number: u32,
    context: &ListenTurnContext<'_>,
) -> RunAgentArgs {
    RunAgentArgs {
        root: Some(context.source_root.to_path_buf()),
        route_key: Some(AGENT_ROUTE_AGENTS_LISTEN.to_string()),
        agent: context.args.agent.clone(),
        prompt: render_review_prompt(issue, turn_number, context),
        instructions: Some(build_review_instructions(context)),
        model: context.args.model.clone(),
        reasoning: context.args.reasoning.clone(),
        transport: None,
        attachments: Vec::new(),
    }
}

fn build_final_review_run_args(
    issue: &IssueSummary,
    turn_number: u32,
    context: &ListenTurnContext<'_>,
    review: &ReviewReport,
) -> RunAgentArgs {
    RunAgentArgs {
        root: Some(context.source_root.to_path_buf()),
        route_key: Some(AGENT_ROUTE_AGENTS_LISTEN.to_string()),
        agent: context.args.agent.clone(),
        prompt: render_final_review_prompt(issue, turn_number, context, review),
        instructions: Some(build_final_review_instructions(context)),
        model: context.args.model.clone(),
        reasoning: context.args.reasoning.clone(),
        transport: None,
        attachments: Vec::new(),
    }
}

fn build_verification_run_args(
    issue: &IssueSummary,
    turn_number: u32,
    context: &ListenTurnContext<'_>,
    quality_criteria: &[String],
    battle_inputs: &[BattleTestInput],
) -> RunAgentArgs {
    RunAgentArgs {
        root: Some(context.source_root.to_path_buf()),
        route_key: Some(AGENT_ROUTE_AGENTS_LISTEN_VERIFICATION.to_string()),
        agent: context.args.agent.clone(),
        prompt: render_verification_prompt(
            issue,
            turn_number,
            context,
            quality_criteria,
            battle_inputs,
        ),
        instructions: Some(build_verification_instructions()),
        model: context.args.model.clone(),
        reasoning: context.args.reasoning.clone(),
        transport: None,
        attachments: Vec::new(),
    }
}

pub(super) fn write_preflight_failure(log_path: &Path, error: &anyhow::Error) -> Result<()> {
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .with_context(|| format!("failed to open `{}`", log_path.display()))?;
    writeln!(
        log,
        "{}{}\n",
        listen_preflight_failure_header(&now_timestamp()),
        error,
    )
    .with_context(|| format!("failed to write `{}`", log_path.display()))
}

struct ExecutionTurnDelta<'a> {
    previous_review: Option<&'a ReviewReport>,
    verification_summary: Option<&'a VerificationSummary>,
}

fn seed_execution_turn_canonical_metadata(
    issue: &IssueSummary,
    turn_number: u32,
    context: &ListenTurnContext<'_>,
    plan: ExecutionTurnPlan,
    delta: ExecutionTurnDelta<'_>,
    continuation_handle: Option<&LatestResumeHandle>,
    canonical: &mut CanonicalSessionData,
) -> Result<()> {
    let effective_agent = resolve_effective_listen_agent(
        context.app_config,
        context.planning_meta,
        context.args.agent.as_deref(),
    );
    let has_resume_handle = continuation_handle
        .filter(|handle| {
            effective_agent
                .as_deref()
                .is_some_and(|agent| handle.matches_agent(agent))
        })
        .is_some();
    let run_args = build_listen_run_args(
        issue,
        turn_number,
        context,
        plan,
        delta.previous_review,
        delta.verification_summary,
        has_resume_handle,
    )?;
    let invocation = resolve_agent_invocation_for_planning(
        context.app_config,
        context.planning_meta,
        &run_args,
    )?;
    canonical.provider = Some(invocation.agent);
    canonical.model = invocation.model;
    canonical.reasoning = invocation.reasoning;
    Ok(())
}

// Execution turns need the current delta, continuation state, and two output callbacks; a local
// lint allowance keeps the call sites direct.
#[allow(clippy::too_many_arguments)]
fn execute_agent_turn(
    issue: &IssueSummary,
    turn_number: u32,
    context: &ListenTurnContext<'_>,
    plan: ExecutionTurnPlan,
    delta: ExecutionTurnDelta<'_>,
    continuation_handle: Option<&LatestResumeHandle>,
    mut on_session_started: impl FnMut(&str) -> Result<()>,
    mut on_usage: impl FnMut(&AgentTokenUsage) -> Result<()>,
) -> Result<TurnExecutionResult> {
    let effective_agent = resolve_effective_listen_agent(
        context.app_config,
        context.planning_meta,
        context.args.agent.as_deref(),
    );
    let has_resume_handle = continuation_handle
        .filter(|h| {
            effective_agent
                .as_deref()
                .is_some_and(|a| h.matches_agent(a))
        })
        .is_some();
    let use_continuation = has_resume_handle && turn_number > 1;
    let prompt_mode = if use_continuation {
        TurnPromptMode::Continuation
    } else {
        TurnPromptMode::FullPrompt
    };
    eprintln!(
        "listen: turn {turn_number}/{} for {} | resume={has_resume_handle} | prompt_mode={}",
        plan.max_turns,
        issue.identifier,
        prompt_mode.label(),
    );
    let run_args = build_listen_run_args(
        issue,
        turn_number,
        context,
        plan,
        delta.previous_review,
        delta.verification_summary,
        has_resume_handle,
    )?;
    execute_agent_run(
        AgentPhaseInvocation {
            issue,
            context,
            turn_number,
            phase_label: "execute",
            prompt_mode,
            capture_response_text: false,
            continuation_handle: if use_continuation {
                continuation_handle
            } else {
                None
            },
        },
        run_args,
        &mut on_session_started,
        &mut on_usage,
    )
}

fn execute_agent_run(
    phase: AgentPhaseInvocation<'_>,
    run_args: RunAgentArgs,
    mut on_session_started: impl FnMut(&str) -> Result<()>,
    mut on_usage: impl FnMut(&AgentTokenUsage) -> Result<()>,
) -> Result<TurnExecutionResult> {
    let issue = phase.issue;
    let context = phase.context;
    let turn_number = phase.turn_number;
    let phase_label = phase.phase_label;
    let prompt_mode = phase.prompt_mode;
    let capture_response_text = phase.capture_response_text;
    let invocation = resolve_agent_invocation_for_planning(
        context.app_config,
        context.planning_meta,
        &run_args,
    )?;
    let capture_output = invocation.builtin_provider || capture_response_text;
    let command_args = if capture_output {
        let continuation =
            continuation_id_for_invocation(&invocation.agent, phase.continuation_handle);
        command_args_for_invocation_with_options(
            &invocation,
            AgentExecutionOptions {
                working_dir: Some(context.workspace_path.to_path_buf()),
                extra_env: Vec::new(),
                capture_output: true,
                continuation,
            },
        )?
    } else {
        command_args_for_invocation(&invocation, Some(context.workspace_path))?
    };
    let attempted_command = validate_invocation_command_surface(&invocation, &command_args)?;
    let log_path = agent_log_path(
        context.source_root,
        context.project_selector,
        &issue.identifier,
    );
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    {
        let mut log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("failed to open `{}`", log_path.display()))?;
        writeln!(
            log,
            "\n--- {} listen {} turn {}/{} @ {} ---",
            crate::branding::COMMAND_NAME,
            phase_label,
            turn_number,
            context.max_turns,
            now_timestamp()
        )
        .with_context(|| format!("failed to write `{}`", log_path.display()))?;
        writeln!(
            log,
            "command: {} {}",
            invocation.command,
            command_args.join(" ")
        )
        .with_context(|| format!("failed to write `{}`", log_path.display()))?;
        for line in render_invocation_diagnostics(&invocation) {
            writeln!(log, "{line}")
                .with_context(|| format!("failed to write `{}`", log_path.display()))?;
        }
    }
    let mut command = Command::new(&invocation.command);
    command.current_dir(context.workspace_path);
    command.args(&command_args);
    apply_noninteractive_agent_environment(&mut command);
    apply_invocation_environment(
        &mut command,
        &invocation,
        &run_args.prompt,
        run_args.instructions.as_deref(),
    );
    command.env("CI", "1");
    command.env("METASTACK_LISTEN_UNATTENDED", "1");
    command.env("METASTACK_LINEAR_ISSUE_ID", &issue.id);
    command.env("METASTACK_LINEAR_ISSUE_IDENTIFIER", &issue.identifier);
    command.env("METASTACK_LINEAR_ISSUE_URL", &issue.url);
    command.env(
        "METASTACK_LINEAR_WORKPAD_COMMENT_ID",
        context.workpad_comment_id,
    );
    command.env("METASTACK_WORKSPACE_PATH", context.workspace_path);
    command.env("METASTACK_SOURCE_ROOT", context.source_root);
    if let Some(backlog_issue) = context.backlog_issue {
        command.env("METASTACK_LINEAR_BACKLOG_ISSUE_ID", &backlog_issue.id);
        command.env(
            "METASTACK_LINEAR_BACKLOG_ISSUE_IDENTIFIER",
            &backlog_issue.identifier,
        );
        command.env("METASTACK_LINEAR_BACKLOG_ISSUE_URL", &backlog_issue.url);
        command.env(
            "METASTACK_LINEAR_BACKLOG_PATH",
            PlanningPaths::new(context.workspace_path).backlog_issue_dir(&backlog_issue.identifier),
        );
    }
    let attachment_context_path =
        PlanningPaths::new(context.workspace_path).agent_issue_context_dir(&issue.identifier);
    if attachment_context_path.is_dir() {
        command.env(
            "METASTACK_LINEAR_ATTACHMENT_CONTEXT_PATH",
            &attachment_context_path,
        );
    }
    for key in [
        "LINEAR_API_KEY",
        "LINEAR_API_URL",
        "LINEAR_TEAM",
        "METASTACK_CONFIG",
    ] {
        if let Ok(value) = std::env::var(key) {
            command.env(key, value);
        }
    }

    match invocation.transport {
        PromptTransport::Arg => {
            command.stdin(Stdio::null());
        }
        PromptTransport::Stdin => {
            command.stdin(Stdio::piped());
        }
    }

    let managed_settings = listen_managed_child_settings(context);
    let managed_stdout = if capture_output {
        ManagedChildOutput::Capture
    } else {
        ManagedChildOutput::File(
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .with_context(|| format!("failed to open `{}`", log_path.display()))?,
        )
    };
    let managed_stderr = if capture_output {
        ManagedChildOutput::Capture
    } else {
        ManagedChildOutput::File(
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .with_context(|| format!("failed to open `{}`", log_path.display()))?,
        )
    };
    let mut child = ManagedChild::spawn(
        &mut command,
        managed_stdout,
        managed_stderr,
        managed_settings,
    )
    .with_context(|| {
        format!(
            "failed to launch agent `{}` with command `{attempted_command}`",
            invocation.agent
        )
    })?;
    let turn_started_at = now_epoch_seconds();

    if invocation.transport == PromptTransport::Stdin {
        let mut stdin = child
            .take_stdin()
            .ok_or_else(|| anyhow!("failed to open stdin for the listen agent turn"))?;
        use std::io::Write as _;
        stdin
            .write_all(invocation.payload.as_bytes())
            .context("failed to write prompt payload to the launched agent")?;
        drop(stdin);
    }

    if capture_output {
        let mut stdout_log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("failed to open `{}`", log_path.display()))?;
        let mut stderr_log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("failed to open `{}`", log_path.display()))?;
        let provider = builtin_provider_adapter(&invocation.agent);
        let mut continuation = None;
        let mut usage = None;
        let mut latest_resume_handle = None;
        let managed_result = child.wait_with_captured_output(
            |line| {
                writeln!(stdout_log, "{line}")
                    .with_context(|| format!("failed to write `{}`", log_path.display()))?;
                if latest_resume_handle.is_none() {
                    latest_resume_handle =
                        parse_resume_handle_line(&invocation.agent, line.as_bytes());
                }
                if let Some(provider) = provider {
                    let parsed = provider.parse_capture_output(line)?;
                    if let Some(current_session_id) = parsed.continuation
                        && continuation.as_deref() != Some(current_session_id.as_str())
                    {
                        on_session_started(&current_session_id)?;
                        continuation = Some(current_session_id);
                    }
                    if let Some(update) = parsed.usage
                        && usage.as_ref() != Some(&update)
                    {
                        on_usage(&update)?;
                        usage = Some(update);
                    }
                }
                Ok(())
            },
            |line| {
                writeln!(stderr_log, "{line}")
                    .with_context(|| format!("failed to write `{}`", log_path.display()))
            },
            |_| Ok(()),
        )?;
        if let Some(timeout) = managed_result.timeout {
            append_agent_turn_timeout_log(
                &log_path,
                phase_label,
                turn_number,
                &invocation.agent,
                timeout,
            )?;
        }
        let raw_stdout = managed_result.stdout.as_deref().unwrap_or_default();
        let stderr_output = managed_result.stderr.as_deref().unwrap_or_default();
        if !managed_result.status.success() && managed_result.timeout.is_none() {
            let code = managed_result
                .status
                .code()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "terminated by signal".to_string());
            bail!(
                "agent `{}` exited unsuccessfully during listen turn {turn_number} ({code}): {}",
                invocation.agent,
                stderr_output.trim()
            );
        }
        let turn_finished_at = now_epoch_seconds();
        let timeout = managed_result
            .timeout
            .as_ref()
            .map(|value| session_timeout_record(turn_number, &managed_result, *value));
        if let Some(provider) = provider {
            let parsed = provider.parse_capture_output(raw_stdout)?;
            return Ok(TurnExecutionResult {
                response_text: parsed.response_text.clone(),
                session_id: parsed.continuation.or(continuation),
                usage: parsed.usage.or(usage),
                timeout,
                latest_resume_handle: latest_resume_handle.or_else(|| {
                    if invocation.agent == "codex" {
                        resolve_codex_resume_handle(
                            context.workspace_path,
                            issue,
                            turn_started_at,
                            turn_finished_at,
                        )
                    } else {
                        None
                    }
                }),
                prompt_mode,
                provider: Some(invocation.agent.clone()),
                model: invocation.model.clone(),
                reasoning: invocation.reasoning.clone(),
            });
        }

        return Ok(TurnExecutionResult {
            response_text: capture_response_text.then(|| raw_stdout.trim().to_string()),
            timeout,
            prompt_mode,
            provider: Some(invocation.agent.clone()),
            model: invocation.model.clone(),
            reasoning: invocation.reasoning.clone(),
            ..TurnExecutionResult::default()
        });
    }

    let managed_result = child.wait(|_| Ok(()))?;
    if let Some(timeout) = managed_result.timeout {
        append_agent_turn_timeout_log(
            &log_path,
            phase_label,
            turn_number,
            &invocation.agent,
            timeout,
        )?;
    }
    if !managed_result.status.success() && managed_result.timeout.is_none() {
        let code = managed_result
            .status
            .code()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "terminated by signal".to_string());
        bail!(
            "agent `{}` exited unsuccessfully during listen turn {turn_number} ({code})",
            invocation.agent
        );
    }

    Ok(TurnExecutionResult {
        timeout: managed_result
            .timeout
            .as_ref()
            .map(|value| session_timeout_record(turn_number, &managed_result, *value)),
        prompt_mode,
        ..TurnExecutionResult::default()
    })
}

fn listen_managed_child_settings(context: &ListenTurnContext<'_>) -> ManagedChildSettings {
    ManagedChildSettings {
        timeout: Duration::from_secs(
            context
                .app_config
                .defaults
                .listen
                .agent_turn_timeout_seconds(),
        ),
        graceful_shutdown: Duration::from_secs(
            context
                .app_config
                .defaults
                .listen
                .agent_graceful_shutdown_seconds(),
        ),
    }
}

fn session_timeout_record(
    turn_number: u32,
    result: &ManagedChildResult,
    timeout: crate::managed_child::ManagedChildTimeout,
) -> SessionTimeoutRecord {
    SessionTimeoutRecord {
        turn: turn_number,
        pid: timeout.pid,
        elapsed_seconds: result.elapsed.as_secs(),
        timeout_seconds: timeout.timeout_seconds(),
        graceful_shutdown_seconds: timeout.graceful_shutdown.as_secs(),
        termination: match timeout.termination {
            ManagedChildTermination::GracefulTerminated => SessionTimeoutTermination::Sigterm,
            ManagedChildTermination::ForceKilled => SessionTimeoutTermination::Sigkill,
        },
    }
}

fn append_agent_turn_timeout_log(
    log_path: &Path,
    phase_label: &str,
    turn_number: u32,
    agent: &str,
    timeout: crate::managed_child::ManagedChildTimeout,
) -> Result<()> {
    append_worker_log(
        log_path,
        "turn timeout",
        &[format!(
            "phase={phase_label} turn={turn_number} agent={agent} pid={} elapsed={}s timeout={}s graceful_shutdown={}s termination={}",
            timeout.pid,
            timeout.elapsed_seconds(),
            timeout.timeout_seconds(),
            timeout.graceful_shutdown.as_secs(),
            timeout.termination.label(),
        )],
    )
}

fn append_turn_token_summary(log_path: &Path, snapshot: &TurnTokenSnapshot) -> Result<()> {
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .with_context(|| format!("failed to open `{}`", log_path.display()))?;
    writeln!(log, "{}", snapshot.display_compact())
        .with_context(|| format!("failed to write `{}`", log_path.display()))
}

fn resolve_effective_listen_agent(
    app_config: &AppConfig,
    planning_meta: &PlanningMeta,
    agent_override: Option<&str>,
) -> Option<String> {
    resolve_agent_config(
        app_config,
        planning_meta,
        Some(AGENT_ROUTE_AGENTS_LISTEN),
        AgentConfigOverrides {
            provider: agent_override.map(String::from),
            ..Default::default()
        },
    )
    .ok()
    .map(|resolved| normalize_agent_name(&resolved.provider))
}

fn continuation_id_for_invocation(
    agent: &str,
    continuation_handle: Option<&LatestResumeHandle>,
) -> Option<String> {
    continuation_handle
        .filter(|handle| handle.matches_agent(agent))
        .map(|handle| handle.id.clone())
}

fn parse_resume_handle_line(agent: &str, line: &[u8]) -> Option<LatestResumeHandle> {
    let trimmed = std::str::from_utf8(line).ok()?.trim();
    if trimmed.is_empty() {
        return None;
    }
    let value: Value = serde_json::from_str(trimmed).ok()?;
    match agent {
        "claude" => parse_claude_resume_handle(&value),
        "codex" => parse_codex_resume_handle(&value),
        _ => None,
    }
}

fn parse_claude_resume_handle(value: &Value) -> Option<LatestResumeHandle> {
    // Claude stream-json wraps each event in an array: [{"type":"system","session_id":"..."}]
    let obj = value
        .as_array()
        .and_then(|arr| arr.first())
        .unwrap_or(value);
    Some(LatestResumeHandle {
        provider: ResumeProvider::Claude,
        id: obj.get("session_id")?.as_str()?.to_string(),
    })
}

fn parse_codex_resume_handle(value: &Value) -> Option<LatestResumeHandle> {
    (value.get("type")?.as_str()? == "thread.started").then_some(LatestResumeHandle {
        provider: ResumeProvider::Codex,
        id: value.get("thread_id")?.as_str()?.to_string(),
    })
}

fn resolve_codex_resume_handle(
    workspace_path: &Path,
    issue: &IssueSummary,
    turn_started_at: u64,
    turn_finished_at: u64,
) -> Option<LatestResumeHandle> {
    let codex_root = codex_root_dir()?;
    let index_candidates =
        read_codex_session_index(&codex_root, turn_started_at, turn_finished_at).ok()?;
    let state_db = latest_codex_state_db(&codex_root)?;
    let rows = query_codex_threads(
        &state_db,
        workspace_path,
        issue,
        turn_started_at,
        turn_finished_at,
        &index_candidates,
    )
    .ok()?;

    (rows.len() == 1).then(|| LatestResumeHandle {
        provider: ResumeProvider::Codex,
        id: rows[0].id.clone(),
    })
}

fn codex_root_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".codex"))
}

fn latest_codex_state_db(codex_root: &Path) -> Option<PathBuf> {
    let mut candidates = fs::read_dir(codex_root)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.starts_with("state_") && value.ends_with(".sqlite"))
        })
        .filter_map(|path| {
            let modified = fs::metadata(&path).ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.0.cmp(&left.0));
    candidates.into_iter().next().map(|(_, path)| path)
}

fn read_codex_session_index(
    codex_root: &Path,
    turn_started_at: u64,
    turn_finished_at: u64,
) -> Result<Vec<String>> {
    let index_path = codex_root.join("session_index.jsonl");
    let contents = fs::read_to_string(&index_path)
        .with_context(|| format!("failed to read `{}`", index_path.display()))?;
    let lower_bound = turn_started_at.saturating_sub(30);
    let upper_bound = turn_finished_at.saturating_add(30);
    let mut ids = Vec::new();

    for line in contents.lines() {
        let entry: CodexSessionIndexEntry = serde_json::from_str(line)
            .with_context(|| format!("failed to decode `{}`", index_path.display()))?;
        let updated_at = DateTime::parse_from_rfc3339(&entry.updated_at)
            .with_context(|| format!("failed to parse `{}` timestamp", entry.updated_at))?
            .with_timezone(&Utc)
            .timestamp();
        if updated_at >= lower_bound as i64 && updated_at <= upper_bound as i64 {
            ids.push(entry.id);
        }
    }

    Ok(ids)
}

fn query_codex_threads(
    state_db: &Path,
    workspace_path: &Path,
    issue: &IssueSummary,
    turn_started_at: u64,
    turn_finished_at: u64,
    recent_ids: &[String],
) -> Result<Vec<CodexThreadRow>> {
    let lower_bound = turn_started_at.saturating_sub(30);
    let upper_bound = turn_finished_at.saturating_add(30);
    let workspace_literal = sqlite_string_literal(&workspace_path.display().to_string());
    let issue_literal = sqlite_string_literal(&issue.identifier);
    let mut clauses = vec![
        "source = 'exec'".to_string(),
        format!("cwd = '{workspace_literal}'"),
        format!("title LIKE '%{issue_literal}%'"),
        format!("created_at >= {lower_bound}"),
        format!("created_at <= {upper_bound}"),
    ];
    if let Ok(branch) = current_workspace_branch(workspace_path)
        && !branch.trim().is_empty()
    {
        clauses.push(format!(
            "git_branch = '{}'",
            sqlite_string_literal(branch.trim())
        ));
    }
    if !recent_ids.is_empty() {
        let ids = recent_ids
            .iter()
            .map(|id| format!("'{}'", sqlite_string_literal(id)))
            .collect::<Vec<_>>()
            .join(", ");
        clauses.push(format!("id IN ({ids})"));
    }
    let query = format!(
        "SELECT id, created_at, updated_at FROM threads WHERE {} ORDER BY updated_at DESC;",
        clauses.join(" AND ")
    );
    let output = Command::new("sqlite3")
        .arg(state_db)
        .arg(&query)
        .output()
        .with_context(|| format!("failed to run `sqlite3 {}`", state_db.display()))?;
    if !output.status.success() {
        bail!(
            "sqlite3 query failed for `{}`: {}",
            state_db.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter_map(CodexThreadRow::from_sqlite_row)
        .collect())
}

fn sqlite_string_literal(value: &str) -> String {
    value.replace('\'', "''")
}

#[derive(Debug, Deserialize)]
struct CodexSessionIndexEntry {
    id: String,
    updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexThreadRow {
    id: String,
}

impl CodexThreadRow {
    fn from_sqlite_row(row: &str) -> Option<Self> {
        let mut parts = row.split('|');
        Some(Self {
            id: parts.next()?.trim().to_string(),
        })
    }
}

fn build_agent_instructions(
    issue: &IssueSummary,
    turn_number: u32,
    context: &ListenTurnContext<'_>,
    plan: ExecutionTurnPlan,
) -> Result<String> {
    let repo_target = RepoTarget::with_workspace(context.source_root, context.workspace_path);
    let workflow_contract = render_workflow_contract_for_listen(context.source_root, repo_target)?;
    let brief_path = PlanningPaths::new(context.workspace_path)
        .agent_briefs_dir
        .join(format!("{}.md", issue.identifier));
    let mut sections = vec![
        workflow_contract,
        format!(
            "You are running inside `{}` listen, an unattended orchestration session.",
            crate::branding::COMMAND_NAME
        ),
        "Never ask a human to perform follow-up actions. Only stop early for a true blocker such as missing required auth, permissions, or secrets.".to_string(),
        "Work only in the provided workspace checkout and do not edit any other filesystem path.".to_string(),
        format!(
            "Use `{}` as the repository root for implementation, validation, commits, pushes, and PR creation.",
            context.workspace_path.display()
        ),
        "Treat the Linear ticket title, description, labels, and attached instructions as the primary work contract. Execute that work directly instead of expanding it into extra planning unless the ticket explicitly asks for that.".to_string(),
        format!(
            "A generated brief is available at `{}` if you need repo context, but do not spend time restating or expanding it unless the ticket requires that depth.",
            brief_path.display()
        ),
        "Keep implementation, validation, and local backlog updates anchored to the provided workspace checkout for the active repository.".to_string(),
        format!(
            "Reconcile the existing `## Codex Workpad` comment `{}` before doing new work and keep that single comment updated in place.",
            context.workpad_comment_id
        ),
        format!(
            "Never overwrite the primary Linear issue description during `{}` listen. Put planning, progress, validation, and status updates in the workpad comment instead.",
            crate::branding::COMMAND_NAME
        ),
        "Execute the requested work directly, validate what you changed, and avoid adding extra planning, analysis, or decomposition unless the ticket explicitly asks for them.".to_string(),
        format!(
            "Each turn must either leave meaningful workspace updates or stop with a concrete blocker. Do not burn turns rewriting `{}/` files, briefs, or workpad notes unless that is part of the ticket's requested deliverable.",
            crate::branding::PROJECT_DIR
        ),
        "If the Linear ticket contains `Validation`, `Test Plan`, or `Testing` sections, mirror them into the workpad and execute them as required checks.".to_string(),
        "Do not consider the task complete until the requested ticket deliverables are committed and pushed. Shared automation will create or update the branch PR as a draft, attach it to Linear, and promote it to ready during the review handoff.".to_string(),
        format!(
            "Shared automation keeps the `{}` label attached when it publishes or updates the GitHub PR for this ticket. If you touch PR metadata directly, preserve that label and do not use the legacy `{}` label.",
            REQUIRED_LISTEN_PR_LABEL, LEGACY_LISTEN_PR_LABEL
        ),
    ];

    if let Some(backlog_issue) = context.backlog_issue {
        sections.push(format!(
            "A local backlog exists for `{}` in `{}`. Use it only as lightweight tracking. Do not expand, rewrite, or improve backlog files unless the ticket explicitly asks for that. If checklist items already exist, mark off only the work you actually completed.",
            backlog_issue.identifier,
            PlanningPaths::new(context.workspace_path)
                .backlog_issue_dir(&backlog_issue.identifier)
                .display()
        ));
    }

    let manifest_path = PlanningPaths::new(context.workspace_path)
        .agent_issue_context_manifest_path(&issue.identifier);
    if manifest_path.is_file() {
        sections.push(format!(
            "Additional Linear attachment context has been downloaded to `{}`. Review `{}` and use the downloaded markdown files and attachments as supporting context before implementation.",
            manifest_path.parent().unwrap_or(context.workspace_path).display(),
            manifest_path.display()
        ));
    }

    if turn_number > 1 {
        sections.push(format!(
            "This is continuation turn {turn_number} of {}. Resume from the current workspace and workpad state instead of restarting from scratch.",
            plan.max_turns
        ));
        sections.push(
            "The previous turn completed normally, but the issue is still active. Do not repeat finished investigation or validation unless the new code changes require it."
                .to_string(),
        );
    }

    if plan.checkpoint_turn {
        sections.push(
            "This is a dedicated context-checkpoint turn. Update `### Review Notes` -> `#### Context Checkpoint` with decisions made, failed approaches, blockers, exact remaining work, and checklist status, and avoid starting broad new implementation work unless it is required to land the checkpoint safely."
                .to_string(),
        );
    }
    if plan.last_execution_turn {
        sections.push(
            "This is the final remaining execution turn before context exhaustion. Prioritize wrap-up, checkpointing, and leaving the branch plus workpad in a resumable state for later review and recovery."
                .to_string(),
        );
    }

    if issue.description.is_none() {
        sections.push(
            "Issue description is empty in Linear; rely on the current workspace and workpad state."
                .to_string(),
        );
    }

    Ok(sections.join("\n\n"))
}

fn build_review_instructions(_: &ListenTurnContext<'_>) -> String {
    format!(
        "You are the review phase for `{}` listen. Review the current workspace against the Linear ticket and return JSON only.\n\nReturn an object with this exact shape:\n{{\n  \"summary\": \"short review summary\",\n  \"complete\": true,\n  \"completed_items\": [\"ticket requirement or deliverable completed\"],\n  \"remaining_items\": [\"specific remaining work item\"],\n  \"validation_completed\": [\"validation step completed\"],\n  \"validation_remaining\": [\"validation still required\"],\n  \"risks\": [\"risk or likely mistake\"],\n  \"notes\": [\"short operator note\"]\n}}\n\nUse the Linear ticket acceptance criteria and validation sections as the source of truth. Treat cross-actor state ownership as a first-class risk: inspect blocked metadata, canonical metadata, `turn_history`, and `latest_resume_handle`; inspect spawn-before-persist flows and whole-state rewrites; and inspect adjacent state-write paths in `src/listen/mod.rs`, `src/listen/store.rs`, and `src/listen/worker.rs` even when they fall outside the immediate diff. Mark `complete` true only when the requested deliverables are done, validation is sufficient, and the branch is ready for final review.",
        crate::branding::COMMAND_NAME
    )
}

fn build_final_review_instructions(_: &ListenTurnContext<'_>) -> String {
    format!(
        "You are the final review phase for `{}` listen. Perform a fast safety review of the current workspace and return JSON only.\n\nReturn an object with this exact shape:\n{{\n  \"approved\": true,\n  \"summary\": \"short final review summary\",\n  \"missing_items\": [\"anything still missing from the ticket\"],\n  \"validation_gaps\": [\"validation still missing\"],\n  \"risks\": [\"residual risk or likely mistake\"],\n  \"notes\": [\"short operator note\"]\n}}\n\nRe-check cross-actor state ownership for blocked metadata, canonical metadata, `turn_history`, and `latest_resume_handle`; re-check spawn-before-persist flows and whole-state rewrites; and re-check the high-risk listen modules `src/listen/mod.rs`, `src/listen/store.rs`, and `src/listen/worker.rs`. Set `approved` true only if the work matches the Linear ticket, acceptance criteria are satisfied, and no material validation gaps remain.",
        crate::branding::COMMAND_NAME
    )
}

fn build_verification_instructions() -> String {
    format!(
        "You are the verification phase for `{}` listen. Perform a strict code-review verification pass and return JSON only.\n\nReturn an object with this exact shape:\n{{\n  \"summary\": \"short verification summary\",\n  \"criteria\": [\n    {{\n      \"name\": \"criterion copied exactly from the prompt\",\n      \"status\": \"passed or failed\",\n      \"summary\": \"short explanation\",\n      \"findings\": [{{\"file\": \"relative/path.rs\", \"line\": 10, \"message\": \"problem detail\"}}],\n      \"remediation\": \"specific fix guidance\"\n    }}\n  ],\n  \"battle_tests\": [\n    {{\n      \"input_path\": \".intuition/verification/inputs/agents.listen/example.md\",\n      \"status\": \"passed or failed\",\n      \"summary\": \"battle-test assessment\",\n      \"remediation\": \"specific follow-up when failed\"\n    }}\n  ],\n  \"notes\": [\"short operator note\"]\n}}\n\nRequirements:\n- Return every quality criterion from the prompt exactly once.\n- Return every battle-test input from the prompt exactly once when any are provided.\n- Use `failed` when you are not confident the branch satisfies a criterion.\n- Provide file and line findings whenever the workspace evidence makes them available.\n- Do not wrap the JSON in markdown fences.",
        crate::branding::COMMAND_NAME
    )
}

// Continuation prompts intentionally keep review, verification, and pressure inputs separate so
// tests can assert each branch directly.
#[allow(clippy::too_many_arguments)]
fn render_execution_delta_prompt(
    issue: &IssueSummary,
    turn_number: u32,
    max_turns: u32,
    previous_review: Option<&ReviewReport>,
    verification_summary: Option<&VerificationSummary>,
    use_continuation: bool,
    _plan: ExecutionTurnPlan,
    pressure_guidance: &str,
) -> String {
    let header = if use_continuation {
        render_continuation_prompt(issue, turn_number, max_turns)
    } else {
        format!(
            "Execution continuation for `{}` turn #{}/{}.\n\nThe previous execution/review cycle did not fully complete the ticket. Resume from the current workspace state using the remaining work below.\n",
            issue.identifier, turn_number, max_turns
        )
    };
    let review_block = previous_review.map(render_review_delta_block).unwrap_or_else(|| {
        "- No prior structured review is available. Resume from the current workspace and workpad state.\n".to_string()
    });
    let verification_block = verification_summary
        .map(render_verification_delta_block)
        .unwrap_or_default();
    let mut prompt = format!(
        "{header}\nRemaining work for `{identifier}`:\n{review_block}{verification_block}\n\nIssue title: {title}\nURL: {url}",
        header = header,
        identifier = issue.identifier,
        review_block = review_block,
        verification_block = verification_block,
        title = issue.title,
        url = issue.url
    );
    if !pressure_guidance.is_empty() {
        prompt.push_str("\n\nContext pressure guidance:\n");
        prompt.push_str(pressure_guidance);
    }
    prompt
}

fn context_pressure_guidance(
    plan: ExecutionTurnPlan,
    context: &ListenTurnContext<'_>,
    use_continuation: bool,
) -> String {
    let mut lines = Vec::new();
    let warning = format!(
        "- Known completed-turn input tokens are approaching the listen context budget ({}/{}).",
        super::format_number(plan.known_input_tokens),
        super::format_number(context.context_budget_tokens)
    );
    match plan.pressure {
        ContextPressure::Elevated if use_continuation => {
            lines.push(format!("- Context pressure: {}.", plan.pressure.label()));
            lines.push(warning);
            lines.push(
                "- Keep changes tight and refresh the workpad with the exact remaining work before ending the turn."
                    .to_string(),
            );
        }
        ContextPressure::High | ContextPressure::Critical if plan.checkpoint_turn => {
            lines.push(format!("- Context pressure: {}.", plan.pressure.label()));
            lines.push(format!(
                "- Known completed-turn input tokens are at or above the checkpoint threshold ({}/{}).",
                super::format_number(plan.known_input_tokens),
                super::format_number(context.context_budget_tokens)
            ));
            lines.push(
                "- Use this turn to create a durable context checkpoint in `### Review Notes` -> `#### Context Checkpoint`: capture decisions made, failed approaches, blockers, exact remaining work, and checklist/validation status."
                    .to_string(),
            );
            lines.push(
                "- Avoid starting broad new implementation unless it is required to leave the branch in a safe resumable state."
                    .to_string(),
            );
        }
        _ => {}
    }
    if plan.last_execution_turn {
        if lines.is_empty() {
            lines.push(format!("- Context pressure: {}.", plan.pressure.label()));
        }
        lines.push(
            "- This is the final remaining execution turn. Prioritize wrap-up and leave the workspace plus workpad ready for review or recovery."
                .to_string(),
        );
    }
    lines.join("\n")
}

fn render_review_delta_block(review: &ReviewReport) -> String {
    let mut lines = Vec::new();
    if !review.completed_items.is_empty() {
        lines.push("Completed:".to_string());
        for item in &review.completed_items {
            lines.push(format!("- {item}"));
        }
    }
    if !review.remaining_items.is_empty() {
        lines.push("Remaining:".to_string());
        for item in &review.remaining_items {
            lines.push(format!("- {item}"));
        }
    }
    if !review.validation_remaining.is_empty() {
        lines.push("Validation still required:".to_string());
        for item in &review.validation_remaining {
            lines.push(format!("- {item}"));
        }
    }
    if !review.risks.is_empty() {
        lines.push("Risks to address:".to_string());
        for item in &review.risks {
            lines.push(format!("- {item}"));
        }
    }
    if !review.notes.is_empty() {
        lines.push("Notes:".to_string());
        for item in &review.notes {
            lines.push(format!("- {item}"));
        }
    }
    if lines.is_empty() {
        "- No explicit remaining work captured.".to_string()
    } else {
        lines.join("\n")
    }
}

fn render_verification_delta_block(summary: &VerificationSummary) -> String {
    let mut lines = vec![
        String::new(),
        "Latest verification:".to_string(),
        format!("- Summary: {}", summary.summary),
        format!("- Status: {}", summary.compact_label()),
    ];
    if !summary.remediation.is_empty() {
        lines.push("Verification remediation:".to_string());
        for item in &summary.remediation {
            lines.push(format!("- {item}"));
        }
    }
    lines.join("\n")
}

fn render_review_prompt(
    issue: &IssueSummary,
    turn_number: u32,
    context: &ListenTurnContext<'_>,
) -> String {
    let acceptance = extract_acceptance_criteria(issue.description.as_deref());
    let validation = extract_validation_requirements(issue.description.as_deref());
    let backlog_path = context.backlog_issue.map(|backlog_issue| {
        PlanningPaths::new(context.workspace_path)
            .backlog_issue_dir(&backlog_issue.identifier)
            .join("index.md")
    });
    format!(
        "Review the current workspace for Linear ticket `{identifier}` after execution turn #{turn_number}.\n\nTicket title: {title}\nTicket URL: {url}\nWorkspace: {workspace}\nWorkpad comment ID: {workpad}\n{backlog}\n\nAcceptance criteria:\n{acceptance}\n\nValidation requirements:\n{validation}\n\nCross-actor state-ownership heuristics:\n{heuristics}\n\nHigh-risk listen modules:\n{modules}\n\nReview the current branch/workspace state against the ticket. Return JSON only.",
        identifier = issue.identifier,
        turn_number = turn_number,
        title = issue.title,
        url = issue.url,
        workspace = context.workspace_path.display(),
        workpad = context.workpad_comment_id,
        backlog = backlog_path
            .map(|path| format!("Backlog index: {}", path.display()))
            .unwrap_or_default(),
        acceptance = render_string_list(
            &acceptance,
            "_No explicit acceptance criteria found in the ticket description._"
        ),
        validation = render_string_list(
            &validation,
            "_No explicit validation section found in the ticket description._"
        ),
        heuristics = render_string_list(&review_focus_items(), "_No extra heuristics._"),
        modules = render_string_list(&high_risk_listen_modules(), "_No high-risk modules._"),
    )
}

fn render_final_review_prompt(
    issue: &IssueSummary,
    turn_number: u32,
    context: &ListenTurnContext<'_>,
    review: &ReviewReport,
) -> String {
    format!(
        "Perform a final safety review for Linear ticket `{identifier}` after execution turn #{turn_number}.\n\nTicket title: {title}\nTicket URL: {url}\nWorkspace: {workspace}\n\nLatest review summary: {summary}\n\nCompleted items:\n{completed}\n\nRemaining items from latest review:\n{remaining}\n\nValidation completed:\n{validation_completed}\n\nValidation remaining:\n{validation_remaining}\n\nCross-actor state-ownership heuristics:\n{heuristics}\n\nHigh-risk listen modules:\n{modules}\n\nReturn JSON only.",
        identifier = issue.identifier,
        turn_number = turn_number,
        title = issue.title,
        url = issue.url,
        workspace = context.workspace_path.display(),
        summary = review.summary,
        completed = render_string_list(&review.completed_items, "_None recorded._"),
        remaining = render_string_list(&review.remaining_items, "_None recorded._"),
        validation_completed = render_string_list(&review.validation_completed, "_None recorded._"),
        validation_remaining = render_string_list(&review.validation_remaining, "_None recorded._"),
        heuristics = render_string_list(&review_focus_items(), "_No extra heuristics._"),
        modules = render_string_list(&high_risk_listen_modules(), "_No high-risk modules._"),
    )
}

#[cfg(test)]
pub(crate) fn review_prompt_hardening_probe() -> (String, String, String, String) {
    let temp = tempfile::tempdir().expect("tempdir should build");
    let workspace = temp.path();
    let source_root = temp.path();
    fs::create_dir_all(workspace.join(".metastack")).expect("metastack dir should build");

    let app_config = AppConfig::default();
    let planning_meta = PlanningMeta::default();
    let args = ListenWorkerArgs {
        source_root: source_root.to_path_buf(),
        project: None,
        workspace: workspace.to_path_buf(),
        issue: "ENG-10749".to_string(),
        workpad_comment_id: "comment-1".to_string(),
        backlog_issue: None,
        max_turns: 20,
        context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
        api_key: None,
        api_url: None,
        profile: None,
        team: None,
        agent: None,
        model: None,
        reasoning: None,
    };
    let context = ListenTurnContext {
        app_config: &app_config,
        planning_meta: &planning_meta,
        args: &args,
        source_root,
        project_selector: None,
        workspace_path: workspace,
        workpad_comment_id: "comment-1",
        backlog_issue: None,
        max_turns: 20,
        context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
    };
    let issue = IssueSummary {
        id: "ENG-10749-id".to_string(),
        identifier: "ENG-10749".to_string(),
        title: "Technical: Harden listen verification and structured review against cross-actor state-ownership regressions".to_string(),
        description: Some(
            "## Acceptance Criteria\n- Preserve listen state.\n\n## Validation\n- cargo test\n"
                .to_string(),
        ),
        url: "https://linear.app/issues/ENG-10749".to_string(),
        priority: None,
        estimate: None,
        updated_at: "2026-03-19T00:00:00Z".to_string(),
        team: crate::linear::TeamRef {
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
    let review = ReviewReport {
        summary: "Review summary".to_string(),
        complete: true,
        completed_items: vec!["Preserved listen state.".to_string()],
        remaining_items: Vec::new(),
        validation_completed: vec!["cargo test".to_string()],
        validation_remaining: Vec::new(),
        risks: Vec::new(),
        notes: Vec::new(),
    };

    (
        build_review_instructions(&context),
        build_final_review_instructions(&context),
        render_review_prompt(&issue, 2, &context),
        render_final_review_prompt(&issue, 2, &context, &review),
    )
}

fn render_verification_prompt(
    issue: &IssueSummary,
    turn_number: u32,
    context: &ListenTurnContext<'_>,
    quality_criteria: &[String],
    battle_inputs: &[BattleTestInput],
) -> String {
    let battle_block = if battle_inputs.is_empty() {
        "_No battle-test inputs were provided for this verification pass._".to_string()
    } else {
        battle_inputs
            .iter()
            .map(|input| {
                format!(
                    "- Input: {}\n  Preview:\n{}\n",
                    input.relative_path,
                    indent_block(&truncate_for_evidence(&input.preview), "    ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "Perform verification for Linear ticket `{identifier}` after final review on execution turn #{turn_number}.\n\nTicket title: {title}\nTicket URL: {url}\nWorkspace: {workspace}\nExecution route being verified: {execution_route}\nVerification route: {verification_route}\n\nQuality criteria:\n{criteria}\n\nBattle-test inputs:\n{battle_inputs}\n\nReview the current workspace state only. Return JSON only.",
        identifier = issue.identifier,
        turn_number = turn_number,
        title = issue.title,
        url = issue.url,
        workspace = context.workspace_path.display(),
        execution_route = AGENT_ROUTE_AGENTS_LISTEN,
        verification_route = AGENT_ROUTE_AGENTS_LISTEN_VERIFICATION,
        criteria = render_string_list(
            quality_criteria,
            "_No explicit verification criteria were provided._"
        ),
        battle_inputs = battle_block,
    )
}

fn render_string_list(values: &[String], empty: &str) -> String {
    if values.is_empty() {
        empty.to_string()
    } else {
        values
            .iter()
            .map(|value| format!("- {value}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn indent_block(value: &str, prefix: &str) -> String {
    value
        .lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_acceptance_criteria(description: Option<&str>) -> Vec<String> {
    extract_markdown_checklist_items(
        description.unwrap_or_default(),
        &["Acceptance Criteria", "Acceptance", "Requirements"],
    )
}

fn extract_validation_requirements(description: Option<&str>) -> Vec<String> {
    extract_markdown_checklist_items(
        description.unwrap_or_default(),
        &["Validation", "Test Plan", "Testing"],
    )
}

fn extract_markdown_checklist_items(body: &str, headings: &[&str]) -> Vec<String> {
    let section = extract_markdown_section(body, headings).unwrap_or_default();
    section
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .collect()
}

fn extract_markdown_section(body: &str, headings: &[&str]) -> Option<String> {
    let normalized_headings = headings
        .iter()
        .map(|heading| heading.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    let mut in_section = false;
    let mut collected = Vec::new();

    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            let normalized = heading.trim().to_ascii_lowercase();
            if normalized_headings
                .iter()
                .any(|candidate| candidate == &normalized)
            {
                in_section = true;
                continue;
            }
            if in_section {
                break;
            }
        }
        if in_section {
            collected.push(line.to_string());
        }
    }

    let section = collected.join("\n").trim().to_string();
    (!section.is_empty()).then_some(section)
}

fn parse_agent_json<T>(raw: &str, phase: &str) -> Result<T>
where
    T: for<'de> serde::Deserialize<'de>,
{
    let trimmed = raw.trim();
    for candidate in parse_json_candidates(trimmed) {
        if let Ok(parsed) = serde_json::from_str::<T>(&candidate) {
            return Ok(parsed);
        }
    }

    bail!(
        "listen {phase} returned invalid JSON: {}",
        preview_text(trimmed)
    )
}

fn parse_json_candidates(raw: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    push_json_candidate(&mut candidates, raw);
    if let Some(stripped) = strip_code_fence(raw) {
        push_json_candidate(&mut candidates, &stripped);
        append_progressive_json_candidates(&mut candidates, &stripped);
    }
    append_progressive_json_candidates(&mut candidates, raw);
    candidates
}

fn append_progressive_json_candidates(candidates: &mut Vec<String>, raw: &str) {
    let Some(end) = raw.rfind('}') else {
        return;
    };
    for (start, character) in raw.char_indices() {
        if character == '{' && start <= end {
            push_json_candidate(candidates, &raw[start..=end]);
        }
    }
}

fn push_json_candidate(candidates: &mut Vec<String>, candidate: &str) {
    if !candidate.is_empty() && !candidates.iter().any(|existing| existing == candidate) {
        candidates.push(candidate.to_string());
    }
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
    const MAX_PREVIEW_LEN: usize = 240;
    if value.chars().count() <= MAX_PREVIEW_LEN {
        value.to_string()
    } else {
        truncate_with_ellipsis(value, MAX_PREVIEW_LEN)
    }
}

async fn run_review_phase(
    issue: &IssueSummary,
    turn_number: u32,
    meaningful_turn_progress: bool,
    turn_progress: &super::TurnProgress,
    context: &ListenTurnContext<'_>,
    phase_context: WorkerPhaseContext<'_>,
) -> Result<ReviewReport> {
    write_listen_session(
        phase_context.source_root,
        phase_context.project_selector,
        build_worker_session(
            issue,
            SessionPhase::Reviewing,
            compact_session_summary([
                Some(format!("Reviewing execution turn {turn_number}")),
                Some(format!("see {}", phase_context.log_path.display())),
            ]),
            phase_context.session_context,
            turn_number,
            phase_context.provider_session_id,
            &phase_context.session_context.canonical,
        ),
    )?;
    let report = if agent_backed_review_enabled() {
        let run_args = build_review_run_args(issue, turn_number, context);
        let result = execute_agent_run(
            AgentPhaseInvocation {
                issue,
                context,
                turn_number,
                phase_label: "review",
                prompt_mode: TurnPromptMode::Continuation,
                capture_response_text: true,
                continuation_handle: None,
            },
            run_args,
            |_| Ok(()),
            |_| Ok(()),
        )?;
        if let Some(timeout) = result.timeout.as_ref() {
            return Err(phase_timeout_error("review", timeout));
        }
        let raw = result
            .response_text
            .ok_or_else(|| anyhow!("listen review did not return any structured output"))?;
        parse_agent_json(&raw, "review")?
    } else {
        heuristic_review_report(
            issue,
            context,
            meaningful_turn_progress,
            turn_progress,
            !matches!(
                phase_context.session_context.pull_request.status,
                PullRequestStatus::Unpublished
            ),
            phase_context.previous_review,
            phase_context.session_context.verification_summary.as_ref(),
        )?
    };
    Ok(report)
}

async fn run_final_review_phase(
    issue: &IssueSummary,
    turn_number: u32,
    context: &ListenTurnContext<'_>,
    review: &ReviewReport,
    phase_context: WorkerPhaseContext<'_>,
) -> Result<FinalReviewReport> {
    write_listen_session(
        phase_context.source_root,
        phase_context.project_selector,
        build_worker_session(
            issue,
            SessionPhase::FinalReview,
            compact_session_summary([
                Some(format!("Final review for execution turn {turn_number}")),
                Some(format!("see {}", phase_context.log_path.display())),
            ]),
            phase_context.session_context,
            turn_number,
            phase_context.provider_session_id,
            &phase_context.session_context.canonical,
        ),
    )?;
    if agent_backed_review_enabled() {
        let run_args = build_final_review_run_args(issue, turn_number, context, review);
        let result = execute_agent_run(
            AgentPhaseInvocation {
                issue,
                context,
                turn_number,
                phase_label: "final-review",
                prompt_mode: TurnPromptMode::Continuation,
                capture_response_text: true,
                continuation_handle: None,
            },
            run_args,
            |_| Ok(()),
            |_| Ok(()),
        )?;
        if let Some(timeout) = result.timeout.as_ref() {
            return Err(phase_timeout_error("final review", timeout));
        }
        let raw = result
            .response_text
            .ok_or_else(|| anyhow!("listen final review did not return any structured output"))?;
        parse_agent_json(&raw, "final review")
    } else {
        heuristic_final_review_report(review)
    }
}

async fn run_verification_phase(
    issue: &IssueSummary,
    turn_number: u32,
    context: &ListenTurnContext<'_>,
    phase_context: WorkerPhaseContext<'_>,
) -> Result<VerificationReport> {
    let verification_settings = &context.app_config.verification;
    let recipe = load_route_verification_recipe(context.workspace_path, AGENT_ROUTE_AGENTS_LISTEN)?;
    let quality_criteria =
        effective_verification_quality_criteria(context.app_config, recipe.as_ref());
    let battle_test_count = verification_settings.battle_test_count();
    let (battle_input_dir, battle_inputs) = discover_battle_test_inputs(
        context.workspace_path,
        AGENT_ROUTE_AGENTS_LISTEN,
        battle_test_count,
    )?;
    let run_args = build_verification_run_args(
        issue,
        turn_number,
        context,
        &quality_criteria,
        &battle_inputs,
    );
    let route = resolve_verification_route_diagnostics(context, &run_args)?;
    write_listen_session(
        phase_context.source_root,
        phase_context.project_selector,
        build_worker_session(
            issue,
            SessionPhase::Verifying,
            compact_session_summary([
                Some(format!(
                    "Verifying turn {turn_number} via {}",
                    route.provider
                )),
                Some(format!("see {}", phase_context.log_path.display())),
            ]),
            phase_context.session_context,
            turn_number,
            phase_context.provider_session_id,
            &phase_context.session_context.canonical,
        ),
    )?;
    append_worker_log(
        phase_context.log_path,
        "verification plan",
        &render_verification_plan_lines(
            &route,
            recipe.as_ref(),
            &quality_criteria,
            battle_test_count,
            &battle_input_dir,
            &battle_inputs,
        ),
    )?;

    let code_review_enabled = verification_settings.code_review_enabled();
    let mut verifier_notes = Vec::new();
    let mut code_review = default_code_review_report(code_review_enabled);
    let mut battle_tests =
        initial_battle_test_report(battle_test_count, &battle_input_dir, &battle_inputs);
    let should_run_verifier = code_review_enabled || !battle_inputs.is_empty();
    if should_run_verifier {
        match execute_agent_run(
            AgentPhaseInvocation {
                issue,
                context,
                turn_number,
                phase_label: "verification",
                prompt_mode: TurnPromptMode::Continuation,
                capture_response_text: true,
                continuation_handle: None,
            },
            run_args,
            |_| Ok(()),
            |_| Ok(()),
        ) {
            Ok(result) if result.timeout.is_some() => {
                verifier_notes.push(
                    phase_timeout_error(
                        "verification",
                        result.timeout.as_ref().expect("timeout should exist"),
                    )
                    .to_string(),
                );
                code_review = failed_code_review_report(
                    code_review_enabled,
                    &quality_criteria,
                    "Verification agent execution timed out before producing a report.",
                    "Repair the verification route or command configuration and rerun verification.",
                );
                battle_tests = failed_battle_test_report(
                    battle_test_count,
                    &battle_input_dir,
                    &battle_inputs,
                    "Verification agent execution timed out before producing battle-test results.",
                    "Repair the verification route or command configuration and rerun verification.",
                );
            }
            Ok(result) => {
                if let Some(raw) = result
                    .response_text
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                {
                    match parse_agent_json::<VerificationAgentOutput>(raw, "verification") {
                        Ok(output) => {
                            if !output.summary.trim().is_empty() {
                                verifier_notes
                                    .push(format!("Verifier summary: {}", output.summary.trim()));
                            }
                            verifier_notes.extend(output.notes.clone());
                            code_review = derive_code_review_report(
                                code_review_enabled,
                                &quality_criteria,
                                &output.criteria,
                            );
                            battle_tests = derive_battle_test_report(
                                battle_test_count,
                                &battle_input_dir,
                                &battle_inputs,
                                &output.battle_tests,
                            );
                        }
                        Err(error) => {
                            verifier_notes.push(error.to_string());
                            code_review = failed_code_review_report(
                                code_review_enabled,
                                &quality_criteria,
                                "Verifier output was malformed; verification failed closed.",
                                "Return valid JSON verification output with every requested criterion.",
                            );
                            battle_tests = failed_battle_test_report(
                                battle_test_count,
                                &battle_input_dir,
                                &battle_inputs,
                                "Verifier output was malformed; verification failed closed.",
                                "Return valid JSON verification output for every sampled battle-test input.",
                            );
                        }
                    }
                } else {
                    verifier_notes.push(
                        "Verifier output was missing; verification failed closed.".to_string(),
                    );
                    code_review = failed_code_review_report(
                        code_review_enabled,
                        &quality_criteria,
                        "Verifier output was missing; verification failed closed.",
                        "Return a structured verification JSON response.",
                    );
                    battle_tests = failed_battle_test_report(
                        battle_test_count,
                        &battle_input_dir,
                        &battle_inputs,
                        "Verifier output was missing; verification failed closed.",
                        "Return a structured verification JSON response for sampled battle-test inputs.",
                    );
                }
            }
            Err(error) => {
                verifier_notes.push(format!("Verification agent execution failed: {error}"));
                code_review = failed_code_review_report(
                    code_review_enabled,
                    &quality_criteria,
                    "Verification agent execution failed before producing a report.",
                    "Repair the verification route or command configuration and rerun verification.",
                );
                battle_tests = failed_battle_test_report(
                    battle_test_count,
                    &battle_input_dir,
                    &battle_inputs,
                    "Verification agent execution failed before producing battle-test results.",
                    "Repair the verification route or command configuration and rerun verification.",
                );
            }
        }
    }

    let e2e = run_e2e_verification(
        context.workspace_path,
        recipe.as_ref(),
        verification_settings.e2e_verification_enabled(),
    )?;
    append_worker_log(
        phase_context.log_path,
        "verification e2e",
        &render_verification_e2e_lines(&e2e),
    )?;
    let quality_gate = exact_sha_quality_gate(context.workspace_path);
    code_review = merge_exact_sha_quality_gate(code_review, &quality_gate);
    append_worker_log(
        phase_context.log_path,
        "verification github quality gate",
        &quality_gate.log_lines,
    )?;
    for line in &quality_gate.log_lines {
        verifier_notes.push(line.clone());
    }

    let status = aggregate_verification_status(code_review.status, e2e.status, battle_tests.status);
    let remediation = collect_verification_remediation(&code_review, &e2e, &battle_tests);
    let summary = render_verification_summary(status, &code_review, &e2e, &battle_tests);
    let report = VerificationReport {
        version: 1,
        issue_identifier: issue.identifier.clone(),
        turn_number,
        generated_at_epoch_seconds: now_epoch_seconds(),
        status,
        summary,
        route: Some(route),
        quality_criteria,
        code_review,
        e2e,
        battle_tests,
        remediation,
        notes: verifier_notes,
    };

    let store = super::store::ListenProjectStore::resolve(
        phase_context.source_root,
        phase_context.project_selector,
    )?;
    store.write_verification_report(&issue.identifier, &report)?;
    Ok(report)
}

fn resolve_verification_route_diagnostics(
    context: &ListenTurnContext<'_>,
    run_args: &RunAgentArgs,
) -> Result<VerificationRouteDiagnostics> {
    let invocation =
        resolve_agent_invocation_for_planning(context.app_config, context.planning_meta, run_args)?;
    Ok(VerificationRouteDiagnostics {
        route_key: invocation
            .route_key
            .clone()
            .unwrap_or_else(|| AGENT_ROUTE_AGENTS_LISTEN_VERIFICATION.to_string()),
        provider: invocation.agent.clone(),
        model: invocation.model.clone(),
        reasoning: invocation.reasoning.clone(),
        provider_source: format_agent_config_source(&invocation.provider_source),
        model_source: invocation
            .model_source
            .as_ref()
            .map(format_agent_config_source),
        reasoning_source: invocation
            .reasoning_source
            .as_ref()
            .map(format_agent_config_source),
    })
}

fn render_verification_plan_lines(
    route: &VerificationRouteDiagnostics,
    recipe: Option<&super::verification::LoadedRouteVerificationRecipe>,
    quality_criteria: &[String],
    battle_test_count: usize,
    battle_input_dir: &Path,
    battle_inputs: &[BattleTestInput],
) -> Vec<String> {
    let mut lines = vec![
        format!("verification_route={}", route.route_key),
        format!("provider={}", route.provider),
        format!("model={}", route.model.as_deref().unwrap_or("unset")),
        format!(
            "reasoning={}",
            route.reasoning.as_deref().unwrap_or("unset")
        ),
        format!("provider_source={}", route.provider_source),
        format!(
            "model_source={}",
            route.model_source.as_deref().unwrap_or("unset")
        ),
        format!(
            "reasoning_source={}",
            route.reasoning_source.as_deref().unwrap_or("unset")
        ),
        format!("quality_criteria={}", quality_criteria.len()),
        format!("battle_test_count={battle_test_count}"),
        format!("battle_input_dir={}", battle_input_dir.display()),
        format!("sampled_battle_inputs={}", battle_inputs.len()),
    ];
    if let Some(recipe) = recipe {
        lines.push(format!("recipe_path={}", recipe.path.display()));
        lines.push(format!("recipe_e2e_steps={}", recipe.recipe.e2e.len()));
    } else {
        lines.push("recipe_path=missing".to_string());
    }
    for criterion in quality_criteria {
        lines.push(format!("criterion={criterion}"));
    }
    for input in battle_inputs {
        lines.push(format!("battle_input={}", input.relative_path));
    }
    lines
}

fn effective_verification_quality_criteria(
    app_config: &AppConfig,
    recipe: Option<&super::verification::LoadedRouteVerificationRecipe>,
) -> Vec<String> {
    if let Some(recipe) = recipe
        && !recipe.recipe.quality_criteria.is_empty()
    {
        return recipe.recipe.quality_criteria.clone();
    }

    let mut criteria = builtin_quality_criteria();
    for criterion in &app_config.verification.quality_criteria {
        push_unique(&mut criteria, criterion.clone());
    }
    criteria
}

fn default_code_review_report(enabled: bool) -> VerificationCodeReviewReport {
    if enabled {
        VerificationCodeReviewReport {
            status: VerificationStatus::Skipped,
            summary: "Awaiting verifier output.".to_string(),
            criteria: Vec::new(),
            notes: Vec::new(),
        }
    } else {
        VerificationCodeReviewReport {
            status: VerificationStatus::Skipped,
            summary: "Code-review verification disabled by install config.".to_string(),
            criteria: Vec::new(),
            notes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExactShaQualityGate {
    criterion: VerificationCriterionResult,
    log_lines: Vec<String>,
}

fn review_focus_items() -> Vec<String> {
    CROSS_ACTOR_REVIEW_HEURISTICS
        .iter()
        .map(|item| item.to_string())
        .collect()
}

fn high_risk_listen_modules() -> Vec<String> {
    HIGH_RISK_LISTEN_MODULES
        .iter()
        .map(|item| item.to_string())
        .collect()
}

fn merge_exact_sha_quality_gate(
    mut code_review: VerificationCodeReviewReport,
    quality_gate: &ExactShaQualityGate,
) -> VerificationCodeReviewReport {
    code_review
        .criteria
        .retain(|criterion| criterion.name != EXACT_SHA_QUALITY_CRITERION);
    code_review.criteria.push(quality_gate.criterion.clone());
    let total = code_review.criteria.len();
    let failures = code_review
        .criteria
        .iter()
        .filter(|criterion| criterion.status == VerificationStatus::Failed)
        .count();
    code_review.status = if failures == 0 {
        VerificationStatus::Passed
    } else {
        VerificationStatus::Failed
    };
    code_review.summary = if failures == 0 {
        format!("Verification approved all {total} criterion/criteria.")
    } else {
        format!("Verification failed {failures} of {total} criterion/criteria.")
    };
    code_review
}

fn exact_sha_quality_gate(workspace_path: &Path) -> ExactShaQualityGate {
    let branch = match super::current_workspace_branch(workspace_path) {
        Ok(branch) => branch,
        Err(error) => {
            return failed_exact_sha_quality_gate(
                format!(
                    "Could not resolve the current workspace branch before checking `{}`.",
                    REQUIRED_VERIFICATION_WORKFLOW_NAME
                ),
                "Repair the workspace Git metadata, then rerun verification.".to_string(),
                vec![format!(
                    "GitHub quality gate blocked: current branch lookup failed: {error}"
                )],
            );
        }
    };
    let local_head_sha = match super::git_stdout(workspace_path, &["rev-parse", "HEAD"]) {
        Ok(head_sha) if !head_sha.trim().is_empty() => head_sha.trim().to_string(),
        Ok(_) => {
            return failed_exact_sha_quality_gate(
                "Could not resolve the local workspace HEAD before checking `quality`.".to_string(),
                "Repair the workspace Git metadata, then rerun verification.".to_string(),
                vec![
                    "GitHub quality gate blocked: local HEAD lookup returned an empty SHA."
                        .to_string(),
                ],
            );
        }
        Err(error) => {
            return failed_exact_sha_quality_gate(
                "Could not resolve the local workspace HEAD before checking `quality`.".to_string(),
                "Repair the workspace Git metadata, then rerun verification.".to_string(),
                vec![format!(
                    "GitHub quality gate blocked: local HEAD lookup failed: {error}"
                )],
            );
        }
    };

    let pull_request = match GhCli.find_open_branch_pull_request(
        workspace_path,
        &branch,
        LISTEN_PULL_REQUEST_BASE_BRANCH,
    ) {
        Ok(Some(pull_request)) => pull_request,
        Ok(None) => {
            return failed_exact_sha_quality_gate(
                format!(
                    "No open branch PR matched `{branch}`; `{}` must pass on the active PR before verification can succeed.",
                    REQUIRED_VERIFICATION_WORKFLOW_NAME
                ),
                format!(
                    "Publish or refresh the branch PR for `{branch}`, wait for `{}` to run on the current head SHA, then rerun verification.",
                    REQUIRED_VERIFICATION_WORKFLOW_NAME
                ),
                vec![format!(
                    "GitHub quality gate blocked: no open PR matched branch `{branch}`."
                )],
            );
        }
        Err(error) => {
            return failed_exact_sha_quality_gate(
                format!(
                    "Could not resolve the active branch PR for `{branch}` before checking `{}`.",
                    REQUIRED_VERIFICATION_WORKFLOW_NAME
                ),
                "Repair GitHub CLI access for the workspace repository, then rerun verification."
                    .to_string(),
                vec![format!(
                    "GitHub quality gate blocked: PR lookup failed: {error}"
                )],
            );
        }
    };

    let Some(head_sha) = pull_request
        .head_ref_oid
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
    else {
        return failed_exact_sha_quality_gate(
            format!(
                "PR #{} did not expose a current HEAD SHA for `{}` verification.",
                pull_request.number, REQUIRED_VERIFICATION_WORKFLOW_NAME
            ),
            format!("Repair GitHub PR metadata for branch `{branch}`, then rerun verification."),
            vec![format!(
                "GitHub quality gate blocked: PR #{} headRefOid was missing.",
                pull_request.number
            )],
        );
    };

    if !local_head_sha.eq_ignore_ascii_case(&head_sha) {
        return failed_exact_sha_quality_gate(
            format!(
                "Local workspace HEAD `{local_head_sha}` does not match active PR #{} head `{head_sha}`.",
                pull_request.number
            ),
            format!(
                "Push branch `{branch}` so PR #{} updates to local HEAD `{local_head_sha}`, then rerun verification.",
                pull_request.number
            ),
            vec![format!(
                "GitHub quality gate blocked: local HEAD `{local_head_sha}` did not match PR #{} head `{head_sha}`.",
                pull_request.number
            )],
        );
    }

    let workflow_runs = match GhCli.list_workflow_runs_for_commit(
        workspace_path,
        REQUIRED_VERIFICATION_WORKFLOW_NAME,
        &head_sha,
    ) {
        Ok(workflow_runs) => workflow_runs,
        Err(error) => {
            return failed_exact_sha_quality_gate(
                format!(
                    "Could not resolve `{}` workflow metadata for PR #{} head `{head_sha}`.",
                    REQUIRED_VERIFICATION_WORKFLOW_NAME, pull_request.number
                ),
                format!(
                    "Repair GitHub Actions metadata access, then rerun verification for PR #{} head `{head_sha}`.",
                    pull_request.number
                ),
                vec![format!(
                    "GitHub quality gate blocked: workflow lookup failed: {error}"
                )],
            );
        }
    };

    evaluate_quality_workflow_runs(&pull_request, &head_sha, workflow_runs)
}

fn evaluate_quality_workflow_runs(
    pull_request: &ResolvedBranchPullRequest,
    head_sha: &str,
    workflow_runs: Vec<WorkflowRun>,
) -> ExactShaQualityGate {
    let Some(latest_run) = workflow_runs.into_iter().next() else {
        return failed_exact_sha_quality_gate(
            format!(
                "No `{}` workflow run was found for PR #{} head `{head_sha}`.",
                REQUIRED_VERIFICATION_WORKFLOW_NAME, pull_request.number
            ),
            format!(
                "Wait for the `{}` workflow to start for PR #{} head `{head_sha}`, then rerun verification.",
                REQUIRED_VERIFICATION_WORKFLOW_NAME, pull_request.number
            ),
            vec![format!(
                "GitHub quality gate blocked: no `{}` runs were returned for PR #{} head `{head_sha}`.",
                REQUIRED_VERIFICATION_WORKFLOW_NAME, pull_request.number
            )],
        );
    };

    let workflow_name = latest_run
        .workflow_name
        .as_deref()
        .unwrap_or(REQUIRED_VERIFICATION_WORKFLOW_NAME);
    let run_url = latest_run.url.as_deref().unwrap_or("unavailable");
    if !latest_run.head_sha.eq_ignore_ascii_case(head_sha)
        || !workflow_name.eq_ignore_ascii_case(REQUIRED_VERIFICATION_WORKFLOW_NAME)
    {
        return failed_exact_sha_quality_gate(
            format!(
                "`{}` workflow metadata did not match PR #{} head `{head_sha}`.",
                REQUIRED_VERIFICATION_WORKFLOW_NAME, pull_request.number
            ),
            format!(
                "Rerun `{}` for PR #{} head `{head_sha}` and confirm the run is attached to the exact current commit.",
                REQUIRED_VERIFICATION_WORKFLOW_NAME, pull_request.number
            ),
            vec![format!(
                "GitHub quality gate blocked: workflow `{workflow_name}` run for head `{}` did not match required head `{head_sha}` (url: {run_url}).",
                latest_run.head_sha
            )],
        );
    }

    if !latest_run.status.eq_ignore_ascii_case("completed") {
        return failed_exact_sha_quality_gate(
            format!(
                "`{}` for PR #{} head `{head_sha}` is still `{}`.",
                REQUIRED_VERIFICATION_WORKFLOW_NAME, pull_request.number, latest_run.status
            ),
            format!(
                "Wait for `{}` to finish successfully on PR #{} head `{head_sha}`, then rerun verification.",
                REQUIRED_VERIFICATION_WORKFLOW_NAME, pull_request.number
            ),
            vec![format!(
                "GitHub quality gate blocked: PR #{} head `{head_sha}` latest `{}` run is `{}` (url: {run_url}).",
                pull_request.number, REQUIRED_VERIFICATION_WORKFLOW_NAME, latest_run.status
            )],
        );
    }

    match latest_run.conclusion.as_deref() {
        Some(conclusion) if conclusion.eq_ignore_ascii_case("success") => {
            passed_exact_sha_quality_gate(
                format!(
                    "`{}` passed for PR #{} head `{head_sha}`.",
                    REQUIRED_VERIFICATION_WORKFLOW_NAME, pull_request.number
                ),
                vec![format!(
                    "GitHub quality gate passed: PR #{} head `{head_sha}` latest `{}` run concluded success (url: {run_url}).",
                    pull_request.number, REQUIRED_VERIFICATION_WORKFLOW_NAME
                )],
            )
        }
        Some(conclusion) => failed_exact_sha_quality_gate(
            format!(
                "`{}` for PR #{} head `{head_sha}` concluded `{conclusion}`.",
                REQUIRED_VERIFICATION_WORKFLOW_NAME, pull_request.number
            ),
            format!(
                "Repair the failing `{}` workflow for PR #{} head `{head_sha}`, then rerun verification.",
                REQUIRED_VERIFICATION_WORKFLOW_NAME, pull_request.number
            ),
            vec![format!(
                "GitHub quality gate blocked: PR #{} head `{head_sha}` latest `{}` run concluded `{conclusion}` (url: {run_url}).",
                pull_request.number, REQUIRED_VERIFICATION_WORKFLOW_NAME
            )],
        ),
        None => failed_exact_sha_quality_gate(
            format!(
                "`{}` for PR #{} head `{head_sha}` completed without a conclusion.",
                REQUIRED_VERIFICATION_WORKFLOW_NAME, pull_request.number
            ),
            format!(
                "Rerun `{}` for PR #{} head `{head_sha}` and wait for a successful conclusion before rerunning verification.",
                REQUIRED_VERIFICATION_WORKFLOW_NAME, pull_request.number
            ),
            vec![format!(
                "GitHub quality gate blocked: PR #{} head `{head_sha}` latest `{}` run completed without a conclusion (url: {run_url}).",
                pull_request.number, REQUIRED_VERIFICATION_WORKFLOW_NAME
            )],
        ),
    }
}

fn passed_exact_sha_quality_gate(summary: String, log_lines: Vec<String>) -> ExactShaQualityGate {
    ExactShaQualityGate {
        criterion: VerificationCriterionResult {
            name: EXACT_SHA_QUALITY_CRITERION.to_string(),
            status: VerificationStatus::Passed,
            summary,
            findings: Vec::new(),
            remediation: None,
        },
        log_lines,
    }
}

fn failed_exact_sha_quality_gate(
    summary: String,
    remediation: String,
    log_lines: Vec<String>,
) -> ExactShaQualityGate {
    ExactShaQualityGate {
        criterion: VerificationCriterionResult {
            name: EXACT_SHA_QUALITY_CRITERION.to_string(),
            status: VerificationStatus::Failed,
            summary,
            findings: Vec::new(),
            remediation: Some(remediation),
        },
        log_lines,
    }
}

fn initial_battle_test_report(
    battle_test_count: usize,
    battle_input_dir: &Path,
    battle_inputs: &[BattleTestInput],
) -> VerificationBattleTestReport {
    if battle_test_count == 0 {
        return VerificationBattleTestReport {
            status: VerificationStatus::Skipped,
            summary: "Battle testing disabled by install config.".to_string(),
            sampled_count: 0,
            cases: Vec::new(),
            input_dir: Some(battle_input_dir.display().to_string()),
        };
    }
    if !battle_input_dir.is_dir() {
        return VerificationBattleTestReport {
            status: VerificationStatus::Skipped,
            summary: format!(
                "Battle-test input directory `{}` is missing.",
                battle_input_dir.display()
            ),
            sampled_count: 0,
            cases: Vec::new(),
            input_dir: Some(battle_input_dir.display().to_string()),
        };
    }
    if battle_inputs.is_empty() {
        return VerificationBattleTestReport {
            status: VerificationStatus::Skipped,
            summary: format!(
                "Battle-test input directory `{}` did not contain any sampled inputs.",
                battle_input_dir.display()
            ),
            sampled_count: 0,
            cases: Vec::new(),
            input_dir: Some(battle_input_dir.display().to_string()),
        };
    }

    VerificationBattleTestReport {
        status: VerificationStatus::Skipped,
        summary: format!(
            "Awaiting verifier results for {} sampled battle-test input(s).",
            battle_inputs.len()
        ),
        sampled_count: battle_inputs.len(),
        cases: battle_inputs
            .iter()
            .map(|input| VerificationBattleTestCase {
                input_path: input.relative_path.clone(),
                status: VerificationStatus::Skipped,
                summary: "Awaiting verifier result.".to_string(),
                remediation: None,
            })
            .collect(),
        input_dir: Some(battle_input_dir.display().to_string()),
    }
}

fn derive_code_review_report(
    code_review_enabled: bool,
    quality_criteria: &[String],
    reported: &[VerificationAgentCriterion],
) -> VerificationCodeReviewReport {
    if !code_review_enabled {
        return default_code_review_report(false);
    }

    let mut criteria = Vec::new();
    let mut failures = 0usize;
    for expected in quality_criteria {
        let criterion = match reported
            .iter()
            .find(|candidate| candidate.name.eq_ignore_ascii_case(expected))
        {
            Some(candidate) if candidate.status == VerificationStatus::Passed => {
                VerificationCriterionResult {
                    name: expected.clone(),
                    status: VerificationStatus::Passed,
                    summary: candidate.summary.clone(),
                    findings: candidate.findings.clone(),
                    remediation: candidate.remediation.clone(),
                }
            }
            Some(candidate) => {
                failures += 1;
                VerificationCriterionResult {
                    name: expected.clone(),
                    status: VerificationStatus::Failed,
                    summary: if candidate.summary.trim().is_empty() {
                        "Verifier did not approve this criterion.".to_string()
                    } else {
                        candidate.summary.clone()
                    },
                    findings: candidate.findings.clone(),
                    remediation: candidate.remediation.clone().or_else(|| {
                        Some("Repair the branch until this criterion clearly passes.".to_string())
                    }),
                }
            }
            None => {
                failures += 1;
                VerificationCriterionResult {
                    name: expected.clone(),
                    status: VerificationStatus::Failed,
                    summary: "Verifier omitted this criterion; verification failed closed."
                        .to_string(),
                    findings: Vec::new(),
                    remediation: Some(
                        "Return an explicit pass/fail result for this criterion.".to_string(),
                    ),
                }
            }
        };
        criteria.push(criterion);
    }

    VerificationCodeReviewReport {
        status: if failures == 0 {
            VerificationStatus::Passed
        } else {
            VerificationStatus::Failed
        },
        summary: if failures == 0 {
            format!(
                "Verifier approved all {} quality criterion/criteria.",
                quality_criteria.len()
            )
        } else {
            format!(
                "Verifier failed {} of {} quality criterion/criteria.",
                failures,
                quality_criteria.len()
            )
        },
        criteria,
        notes: Vec::new(),
    }
}

fn failed_code_review_report(
    code_review_enabled: bool,
    quality_criteria: &[String],
    summary: &str,
    remediation: &str,
) -> VerificationCodeReviewReport {
    if !code_review_enabled {
        return default_code_review_report(false);
    }

    VerificationCodeReviewReport {
        status: VerificationStatus::Failed,
        summary: summary.to_string(),
        criteria: quality_criteria
            .iter()
            .map(|criterion| VerificationCriterionResult {
                name: criterion.clone(),
                status: VerificationStatus::Failed,
                summary: summary.to_string(),
                findings: Vec::new(),
                remediation: Some(remediation.to_string()),
            })
            .collect(),
        notes: Vec::new(),
    }
}

fn derive_battle_test_report(
    battle_test_count: usize,
    battle_input_dir: &Path,
    battle_inputs: &[BattleTestInput],
    reported: &[VerificationAgentBattleTest],
) -> VerificationBattleTestReport {
    if battle_inputs.is_empty() {
        return initial_battle_test_report(battle_test_count, battle_input_dir, battle_inputs);
    }

    let mut failures = 0usize;
    let mut cases = Vec::new();
    for expected in battle_inputs {
        let case = match reported
            .iter()
            .find(|candidate| candidate.input_path == expected.relative_path)
        {
            Some(candidate) if candidate.status == VerificationStatus::Passed => {
                VerificationBattleTestCase {
                    input_path: expected.relative_path.clone(),
                    status: VerificationStatus::Passed,
                    summary: candidate.summary.clone(),
                    remediation: candidate.remediation.clone(),
                }
            }
            Some(candidate) => {
                failures += 1;
                VerificationBattleTestCase {
                    input_path: expected.relative_path.clone(),
                    status: VerificationStatus::Failed,
                    summary: if candidate.summary.trim().is_empty() {
                        "Verifier did not approve this battle-test input.".to_string()
                    } else {
                        candidate.summary.clone()
                    },
                    remediation: candidate.remediation.clone().or_else(|| {
                        Some(
                            "Repair the branch until this sampled battle-test input passes."
                                .to_string(),
                        )
                    }),
                }
            }
            None => {
                failures += 1;
                VerificationBattleTestCase {
                    input_path: expected.relative_path.clone(),
                    status: VerificationStatus::Failed,
                    summary: "Verifier omitted this battle-test input; verification failed closed."
                        .to_string(),
                    remediation: Some(
                        "Return an explicit pass/fail result for this sampled input.".to_string(),
                    ),
                }
            }
        };
        cases.push(case);
    }

    VerificationBattleTestReport {
        status: if failures == 0 {
            VerificationStatus::Passed
        } else {
            VerificationStatus::Failed
        },
        summary: if failures == 0 {
            format!(
                "Battle tests passed for {} sampled input(s).",
                battle_inputs.len()
            )
        } else {
            format!(
                "Battle tests failed for {} of {} sampled input(s).",
                failures,
                battle_inputs.len()
            )
        },
        sampled_count: battle_inputs.len(),
        cases,
        input_dir: Some(battle_input_dir.display().to_string()),
    }
}

fn failed_battle_test_report(
    battle_test_count: usize,
    battle_input_dir: &Path,
    battle_inputs: &[BattleTestInput],
    summary: &str,
    remediation: &str,
) -> VerificationBattleTestReport {
    if battle_inputs.is_empty() {
        return initial_battle_test_report(battle_test_count, battle_input_dir, battle_inputs);
    }

    VerificationBattleTestReport {
        status: VerificationStatus::Failed,
        summary: summary.to_string(),
        sampled_count: battle_inputs.len(),
        cases: battle_inputs
            .iter()
            .map(|input| VerificationBattleTestCase {
                input_path: input.relative_path.clone(),
                status: VerificationStatus::Failed,
                summary: summary.to_string(),
                remediation: Some(remediation.to_string()),
            })
            .collect(),
        input_dir: Some(battle_input_dir.display().to_string()),
    }
}

fn run_e2e_verification(
    workspace_path: &Path,
    recipe: Option<&super::verification::LoadedRouteVerificationRecipe>,
    enabled: bool,
) -> Result<VerificationE2eReport> {
    if !enabled {
        return Ok(VerificationE2eReport {
            status: VerificationStatus::Skipped,
            summary: "E2E verification disabled by install config.".to_string(),
            recipe_path: recipe.map(|loaded| loaded.path.display().to_string()),
            steps: Vec::new(),
        });
    }
    let Some(recipe) = recipe else {
        return Ok(VerificationE2eReport {
            status: VerificationStatus::Skipped,
            summary: "No route-scoped verification recipe was found.".to_string(),
            recipe_path: None,
            steps: Vec::new(),
        });
    };
    if recipe.recipe.e2e.is_empty() {
        return Ok(VerificationE2eReport {
            status: VerificationStatus::Skipped,
            summary: format!(
                "Verification recipe `{}` does not define any E2E steps.",
                recipe.path.display()
            ),
            recipe_path: Some(recipe.path.display().to_string()),
            steps: Vec::new(),
        });
    }

    let mut steps = Vec::new();
    let mut failures = 0usize;
    for step in &recipe.recipe.e2e {
        let report = run_e2e_recipe_step(workspace_path, step)?;
        if report.status == VerificationStatus::Failed {
            failures += 1;
        }
        steps.push(report);
    }

    Ok(VerificationE2eReport {
        status: if failures == 0 {
            VerificationStatus::Passed
        } else {
            VerificationStatus::Failed
        },
        summary: if failures == 0 {
            format!("E2E verification passed for {} step(s).", steps.len())
        } else {
            format!(
                "E2E verification failed for {} of {} step(s).",
                failures,
                steps.len()
            )
        },
        recipe_path: Some(recipe.path.display().to_string()),
        steps,
    })
}

fn run_e2e_recipe_step(
    workspace_path: &Path,
    step: &VerificationRecipeStep,
) -> Result<VerificationE2eStepReport> {
    run_e2e_recipe_step_with_timeout(
        workspace_path,
        step,
        Duration::from_secs(step.timeout_seconds()),
    )
}

fn run_e2e_recipe_step_with_timeout(
    workspace_path: &Path,
    step: &VerificationRecipeStep,
    timeout: Duration,
) -> Result<VerificationE2eStepReport> {
    if step.command.is_empty() {
        return Ok(VerificationE2eStepReport {
            name: step.name.clone(),
            command: Vec::new(),
            timeout_seconds: Some(timeout.as_secs()),
            elapsed_seconds: None,
            timed_out: false,
            status: VerificationStatus::Failed,
            exit_code: None,
            assertions: vec!["recipe step must define at least one command token".to_string()],
            stdout_excerpt: None,
            stderr_excerpt: None,
        });
    }

    let output = run_e2e_recipe_command(workspace_path, step, timeout)?;
    let mut assertions = Vec::new();
    if output.timed_out {
        assertions.push(format!(
            "step timed out after {} (elapsed {}s)",
            format_duration_label(timeout),
            output.elapsed_seconds
        ));
    } else {
        let expected_exit_code = step.expect_exit_code.unwrap_or(0);
        if output.exit_code != Some(expected_exit_code) {
            assertions.push(format!(
                "expected exit code {expected_exit_code}, observed {}",
                output
                    .exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "signal".to_string())
            ));
        }
    }
    for expected in &step.expect_stdout_contains {
        if !output.stdout.contains(expected) {
            assertions.push(format!("stdout must contain `{expected}`"));
        }
    }
    for expected in &step.expect_stderr_contains {
        if !output.stderr.contains(expected) {
            assertions.push(format!("stderr must contain `{expected}`"));
        }
    }
    for expected in &step.expect_paths_exist {
        let resolved = resolve_workspace_assertion_path(workspace_path, expected)?;
        if !resolved.exists() {
            assertions.push(format!("expected path `{expected}` to exist"));
        }
    }
    for expected in &step.expect_paths_missing {
        let resolved = resolve_workspace_assertion_path(workspace_path, expected)?;
        if resolved.exists() {
            assertions.push(format!("expected path `{expected}` to be absent"));
        }
    }

    Ok(VerificationE2eStepReport {
        name: step.name.clone(),
        command: step.command.clone(),
        timeout_seconds: Some(timeout.as_secs()),
        elapsed_seconds: Some(output.elapsed_seconds),
        timed_out: output.timed_out,
        status: if assertions.is_empty() {
            VerificationStatus::Passed
        } else {
            VerificationStatus::Failed
        },
        exit_code: output.exit_code,
        assertions,
        stdout_excerpt: output_excerpt(&output.stdout),
        stderr_excerpt: output_excerpt(&output.stderr),
    })
}

struct RecipeStepCommandOutput {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
    elapsed_seconds: u64,
}

fn run_e2e_recipe_command(
    workspace_path: &Path,
    step: &VerificationRecipeStep,
    timeout: Duration,
) -> Result<RecipeStepCommandOutput> {
    let mut command = Command::new(&step.command[0]);
    command.args(&step.command[1..]);
    command.current_dir(workspace_path);
    let child = ManagedChild::spawn(
        &mut command,
        ManagedChildOutput::Capture,
        ManagedChildOutput::Capture,
        ManagedChildSettings {
            timeout,
            graceful_shutdown: Duration::from_secs(5),
        },
    )
    .with_context(|| {
        format!(
            "failed to run verification recipe step `{}` in `{}`",
            step.name,
            workspace_path.display()
        )
    })?;
    let output = child
        .wait_with_captured_output(|_| Ok(()), |_| Ok(()), |_| Ok(()))
        .with_context(|| {
            format!(
                "failed to wait for verification recipe step `{}` in `{}`",
                step.name,
                workspace_path.display()
            )
        })?;

    Ok(RecipeStepCommandOutput {
        exit_code: output.status.code(),
        stdout: output.stdout.unwrap_or_default(),
        stderr: output.stderr.unwrap_or_default(),
        timed_out: output.timeout.is_some(),
        elapsed_seconds: output.elapsed.as_secs(),
    })
}

fn format_duration_label(timeout: Duration) -> String {
    if timeout.subsec_millis() == 0 && timeout.as_secs() > 0 {
        format!("{}s", timeout.as_secs())
    } else {
        format!("{}ms", timeout.as_millis())
    }
}

fn truncate_with_ellipsis(value: &str, max_chars: usize) -> String {
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    let mut truncated = value.chars().take(max_chars - 3).collect::<String>();
    truncated.push_str("...");
    truncated
}

fn resolve_workspace_assertion_path(workspace_path: &Path, candidate: &str) -> Result<PathBuf> {
    let candidate_path = Path::new(candidate);
    if candidate_path.is_absolute() {
        bail!("verification recipe paths must be workspace-relative: `{candidate}`");
    }

    let mut relative = PathBuf::new();
    for component in candidate_path.components() {
        match component {
            std::path::Component::Normal(value) => relative.push(value),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                bail!("verification recipe paths must stay within the workspace: `{candidate}`")
            }
        }
    }

    if relative.as_os_str().is_empty() {
        bail!("verification recipe path cannot be empty");
    }
    Ok(workspace_path.join(relative))
}

fn render_verification_e2e_lines(report: &VerificationE2eReport) -> Vec<String> {
    let mut lines = vec![
        format!("status={}", report.status.label()),
        format!("summary={}", report.summary),
    ];
    if let Some(recipe_path) = report.recipe_path.as_deref() {
        lines.push(format!("recipe_path={recipe_path}"));
    }
    for step in &report.steps {
        lines.push(format!(
            "step={} status={} exit_code={} timeout_seconds={} elapsed_seconds={} timed_out={}",
            step.name,
            step.status.label(),
            step.exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_string()),
            step.timeout_seconds
                .map(|seconds| seconds.to_string())
                .unwrap_or_else(|| "unset".to_string()),
            step.elapsed_seconds
                .map(|seconds| seconds.to_string())
                .unwrap_or_else(|| "unset".to_string()),
            step.timed_out
        ));
        for assertion in &step.assertions {
            lines.push(format!("assertion={assertion}"));
        }
        if let Some(stdout) = step.stdout_excerpt.as_deref() {
            lines.push(format!("stdout={stdout}"));
        }
        if let Some(stderr) = step.stderr_excerpt.as_deref() {
            lines.push(format!("stderr={stderr}"));
        }
    }
    lines
}

fn aggregate_verification_status(
    code_review: VerificationStatus,
    e2e: VerificationStatus,
    battle_tests: VerificationStatus,
) -> VerificationStatus {
    if [code_review, e2e, battle_tests]
        .into_iter()
        .any(|status| status == VerificationStatus::Failed)
    {
        VerificationStatus::Failed
    } else if [code_review, e2e, battle_tests]
        .into_iter()
        .any(|status| status == VerificationStatus::Passed)
    {
        VerificationStatus::Passed
    } else {
        VerificationStatus::Skipped
    }
}

fn render_verification_summary(
    status: VerificationStatus,
    code_review: &VerificationCodeReviewReport,
    e2e: &VerificationE2eReport,
    battle_tests: &VerificationBattleTestReport,
) -> String {
    let mut components = Vec::new();
    for (component_status, component_summary) in [
        (code_review.status, code_review.summary.as_str()),
        (e2e.status, e2e.summary.as_str()),
        (battle_tests.status, battle_tests.summary.as_str()),
    ] {
        if component_status != VerificationStatus::Skipped && !component_summary.trim().is_empty() {
            components.push(component_summary.trim().to_string());
        }
    }

    let prefix = match status {
        VerificationStatus::Passed => "Verification passed.",
        VerificationStatus::Failed => "Verification failed.",
        VerificationStatus::Skipped => "Verification skipped.",
    };
    if components.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix} {}", components.join(" "))
    }
}

fn collect_verification_remediation(
    code_review: &VerificationCodeReviewReport,
    e2e: &VerificationE2eReport,
    battle_tests: &VerificationBattleTestReport,
) -> Vec<String> {
    let mut remediation = Vec::new();
    for criterion in &code_review.criteria {
        if let Some(item) = criterion.remediation.as_deref()
            && criterion.status == VerificationStatus::Failed
        {
            push_unique(&mut remediation, item.to_string());
        }
    }
    for step in &e2e.steps {
        if step.status == VerificationStatus::Failed {
            for assertion in &step.assertions {
                push_unique(
                    &mut remediation,
                    format!("Repair E2E step `{}`: {assertion}", step.name),
                );
            }
        }
    }
    for case in &battle_tests.cases {
        if let Some(item) = case.remediation.as_deref()
            && case.status == VerificationStatus::Failed
        {
            push_unique(&mut remediation, item.to_string());
        }
    }
    remediation
}

async fn sync_review_tracking(
    service: &LinearService<ReqwestLinearClient>,
    issue: &IssueSummary,
    context: &ListenTurnContext<'_>,
    app_config: &AppConfig,
    session_context: &mut WorkerSessionContext<'_>,
    log_path: &Path,
    review: &ReviewReport,
) -> Result<()> {
    sync_review_tracking_with_checkpoint(
        service,
        issue,
        context,
        app_config,
        session_context,
        log_path,
        review,
        None,
    )
    .await
}

// Review sync needs the service, current issue/context, mutable session state, and optional
// checkpoint payload at the same call site.
#[allow(clippy::too_many_arguments)]
async fn sync_review_tracking_with_checkpoint(
    service: &LinearService<ReqwestLinearClient>,
    issue: &IssueSummary,
    context: &ListenTurnContext<'_>,
    app_config: &AppConfig,
    session_context: &mut WorkerSessionContext<'_>,
    log_path: &Path,
    review: &ReviewReport,
    context_checkpoint: Option<&str>,
) -> Result<()> {
    let body = render_review_workpad(issue, context, review, session_context, context_checkpoint);
    let mut updated = false;
    let mut sync_error = None;
    for attempt in 0..3 {
        match service
            .update_workpad_comment_by_id(context.workpad_comment_id, body.clone())
            .await
        {
            Ok(_) => {
                updated = true;
                break;
            }
            Err(error) if attempt < 2 && is_transient_linear_read_failure(&error) => {
                sleep(Duration::from_millis(100)).await;
                sync_error = Some(error);
            }
            Err(error) => {
                sync_error = Some(error);
                break;
            }
        }
    }
    if !updated {
        for upsert_attempt in 0..3 {
            match service.upsert_workpad_comment(issue, body.clone()).await {
                Ok(_) => {
                    updated = true;
                    break;
                }
                Err(error) if upsert_attempt < 2 && is_transient_linear_read_failure(&error) => {
                    sleep(Duration::from_millis(100)).await;
                    sync_error = Some(error);
                }
                Err(error) => {
                    sync_error = Some(error);
                    break;
                }
            }
        }
    }
    if updated {
        update_pending_linear_sync(&mut session_context.pending_linear_sync, |pending| {
            pending.workpad_body = None;
        });
    } else if let Some(error) = sync_error {
        defer_pending_linear_sync_operation(
            app_config,
            &mut session_context.pending_linear_sync,
            &error,
            "workpad sync",
            log_path,
            |pending| {
                pending.workpad_body = Some(body.clone());
            },
        )?;
    } else {
        update_pending_linear_sync(&mut session_context.pending_linear_sync, |pending| {
            pending.workpad_body = Some(body.clone());
        });
        append_worker_log(
            log_path,
            "pending linear sync",
            &["Deferred workpad sync without a captured Linear error".to_string()],
        )?;
    }
    if let Some(backlog_issue) = context.backlog_issue {
        sync_backlog_progress_section(context.workspace_path, &backlog_issue.identifier, review)?;
    }
    Ok(())
}

fn render_review_workpad(
    issue: &IssueSummary,
    context: &ListenTurnContext<'_>,
    review: &ReviewReport,
    session_context: &WorkerSessionContext<'_>,
    context_checkpoint: Option<&str>,
) -> String {
    let effective_workpad = effective_workpad_body(
        issue,
        session_context.pending_linear_sync.as_ref(),
        context.workpad_comment_id,
    );
    let preserved_review_notes = effective_workpad
        .as_deref()
        .map(extract_unmanaged_review_notes)
        .unwrap_or_default();
    let preserved_checkpoint = context_checkpoint.map(str::to_string).or_else(|| {
        effective_workpad
            .as_deref()
            .and_then(extract_context_checkpoint)
    });
    let mut lines = vec![
        "## Codex Workpad".to_string(),
        String::new(),
        format!("- Ticket: `{}`", issue.identifier),
        format!("- Workspace: `{}`", context.workspace_path.display()),
        format!("- Summary: {}", review.summary),
        format!(
            "- Completion status: {}",
            if review.complete {
                "complete"
            } else {
                "incomplete"
            }
        ),
        String::new(),
        "### Completed".to_string(),
        String::new(),
    ];
    if review.completed_items.is_empty() {
        lines.push("- [ ] No completed items recorded yet.".to_string());
    } else {
        for item in &review.completed_items {
            lines.push(format!("- [x] {item}"));
        }
    }
    lines.extend([String::new(), "### Remaining".to_string(), String::new()]);
    if review.remaining_items.is_empty() {
        lines.push("- [x] No remaining items identified.".to_string());
    } else {
        for item in &review.remaining_items {
            lines.push(format!("- [ ] {item}"));
        }
    }
    lines.extend([String::new(), "### Validation".to_string(), String::new()]);
    for item in &review.validation_completed {
        lines.push(format!("- [x] {item}"));
    }
    for item in &review.validation_remaining {
        lines.push(format!("- [ ] {item}"));
    }
    if review.validation_completed.is_empty() && review.validation_remaining.is_empty() {
        lines.push("- [ ] No explicit validation status recorded.".to_string());
    }
    if let Some(verification) = session_context.verification_summary.as_ref() {
        lines.extend([String::new(), "### Verification".to_string(), String::new()]);
        lines.push(format!("- Status: {}", verification.status.display_label()));
        lines.push(format!("- Summary: {}", verification.summary));
        if verification.criteria_total > 0 {
            lines.push(format!(
                "- Criteria failures: {}/{}",
                verification.criteria_failed, verification.criteria_total
            ));
        }
        lines.push(format!(
            "- E2E: {}",
            verification.e2e_status.display_label()
        ));
        lines.push(format!(
            "- Battle tests: {}",
            verification.battle_test_status.display_label()
        ));
        for item in &verification.remediation {
            lines.push(format!("- [ ] {item}"));
        }
    }
    let visible_notes = review.notes.iter().collect::<Vec<_>>();
    if !review.risks.is_empty()
        || !visible_notes.is_empty()
        || !preserved_review_notes.is_empty()
        || preserved_checkpoint.is_some()
    {
        lines.extend([String::new(), "### Review Notes".to_string(), String::new()]);
        for item in &review.risks {
            lines.push(format!("- Risk: {item}"));
        }
        for item in visible_notes {
            lines.push(format!("- Note: {item}"));
        }
        if !preserved_review_notes.is_empty() {
            if !review.risks.is_empty() || !review.notes.is_empty() {
                lines.push(String::new());
            }
            lines.extend(preserved_review_notes);
        }
        if let Some(checkpoint) = preserved_checkpoint {
            if !review.risks.is_empty()
                || !review.notes.is_empty()
                || !lines.last().is_some_and(String::is_empty)
            {
                lines.push(String::new());
            }
            lines.extend(checkpoint.lines().map(str::to_string));
        }
    }
    lines.join("\n")
}

fn block_for_context_pressure_limit(
    source_root: &Path,
    project_selector: Option<&str>,
    issue: &IssueSummary,
    session_context: &WorkerSessionContext<'_>,
    turns_completed: u32,
    provider_session_id: Option<&str>,
    log_path: &Path,
) -> Result<()> {
    let blocked = blocked_reason(
        BlockedCategory::Turn,
        "critical context pressure reached the final execution turn",
        false,
    );
    write_listen_session(
        source_root,
        project_selector,
        build_worker_blocked_session(
            issue,
            blocked.clone(),
            compact_session_summary([
                Some(blocked.summary_headline()),
                Some(
                    "execution turn limit reduced to one under critical context pressure"
                        .to_string(),
                ),
                Some(format!("see {}", log_path.display())),
            ]),
            session_context,
            turns_completed,
            provider_session_id,
            &session_context.canonical,
        ),
    )
}

fn agent_backed_review_enabled() -> bool {
    false
}

fn heuristic_review_report(
    issue: &IssueSummary,
    context: &ListenTurnContext<'_>,
    _meaningful_turn_progress: bool,
    turn_progress: &super::TurnProgress,
    has_existing_pull_request: bool,
    previous_review: Option<&ReviewReport>,
    verification_summary: Option<&VerificationSummary>,
) -> Result<ReviewReport> {
    let acceptance = extract_acceptance_criteria(issue.description.as_deref());
    let validation = extract_validation_requirements(issue.description.as_deref());
    let backlog_progress = context
        .backlog_issue
        .as_ref()
        .map(|backlog_issue| {
            backlog_progress_for_issue_dir(context.workspace_path, &backlog_issue.identifier)
        })
        .transpose()?;
    let backlog_complete = backlog_progress
        .as_ref()
        .is_some_and(|progress| progress.total > 0 && progress.completed == progress.total);
    let complete_from_retry = has_existing_pull_request
        && acceptance.is_empty()
        && validation.is_empty()
        && (verification_summary
            .is_some_and(|summary| summary.status == VerificationStatus::Failed)
            || previous_review.is_some_and(|review| !review.validation_remaining.is_empty()));
    let complete = if backlog_progress
        .as_ref()
        .is_some_and(|progress| progress.total > 0)
    {
        backlog_complete || complete_from_retry
    } else {
        // Without a backlog checklist the heuristic still does not have enough
        // signal to declare completion on its own. The only exception is an
        // explicit gate-repair retry, where a stored failed verification
        // summary or the previous review's validation-repair context provides
        // the missing readiness signal.
        complete_from_retry
    };
    let changed_items = turn_progress
        .implementation_entries
        .iter()
        .chain(turn_progress.planning_entries.iter())
        .map(|entry| format!("Changed `{entry}`"))
        .collect::<Vec<_>>();
    let completed_items = if complete {
        if acceptance.is_empty() {
            if changed_items.is_empty() {
                vec![
                    "Workspace changes are present and no explicit acceptance criteria remain."
                        .to_string(),
                ]
            } else {
                changed_items.clone()
            }
        } else {
            acceptance.clone()
        }
    } else {
        changed_items.clone()
    };
    let mut remaining_items = if complete {
        Vec::new()
    } else if let Some(progress) = backlog_progress.as_ref() {
        progress.next_step.clone().into_iter().collect::<Vec<_>>()
    } else {
        acceptance.clone()
    };
    if remaining_items.is_empty() && !complete && !acceptance.is_empty() {
        remaining_items = acceptance.clone();
    }

    Ok(ReviewReport {
        summary: if complete {
            "Heuristic review believes the ticket work is complete.".to_string()
        } else if !changed_items.is_empty() {
            "Heuristic review detected branch changes, but additional ticket work remains."
                .to_string()
        } else {
            "Heuristic review found remaining work.".to_string()
        },
        complete,
        completed_items,
        remaining_items,
        validation_completed: if complete && !validation.is_empty() {
            validation.clone()
        } else {
            Vec::new()
        },
        validation_remaining: if complete { Vec::new() } else { validation },
        risks: Vec::new(),
        notes: vec![
            "Using heuristic review; dedicated code verification runs in the verification phase."
                .to_string(),
        ],
    })
}

fn heuristic_final_review_report(review: &ReviewReport) -> Result<FinalReviewReport> {
    Ok(FinalReviewReport {
        approved: review.complete && review.validation_remaining.is_empty(),
        summary: if review.complete && review.validation_remaining.is_empty() {
            "Heuristic final review approved the ticket.".to_string()
        } else {
            "Heuristic final review found missing work or validation gaps.".to_string()
        },
        missing_items: review.remaining_items.clone(),
        validation_gaps: review.validation_remaining.clone(),
        risks: review.risks.clone(),
        notes: vec![
            "Using heuristic final review; dedicated code verification runs in the verification phase."
                .to_string(),
        ],
    })
}

fn sync_backlog_progress_section(
    workspace_path: &Path,
    identifier: &str,
    review: &ReviewReport,
) -> Result<()> {
    let path = PlanningPaths::new(workspace_path)
        .backlog_issue_dir(identifier)
        .join("index.md");
    if !path.is_file() {
        return Ok(());
    }
    let existing = fs::read_to_string(&path)
        .with_context(|| format!("failed to read `{}`", path.display()))?;
    let rendered = render_backlog_progress_section(review);
    let updated = upsert_marked_section(&existing, "metastack-listen-progress", &rendered);
    write_text_file(&path, &updated, true)?;
    Ok(())
}

fn render_backlog_progress_section(review: &ReviewReport) -> String {
    let mut lines = vec![
        "## Listener Progress Checklist".to_string(),
        String::new(),
        "### Completed".to_string(),
        String::new(),
    ];
    if review.completed_items.is_empty() {
        lines.push("- [ ] No completed items recorded yet.".to_string());
    } else {
        for item in &review.completed_items {
            lines.push(format!("- [x] {item}"));
        }
    }
    lines.extend([String::new(), "### Remaining".to_string(), String::new()]);
    if review.remaining_items.is_empty() {
        lines.push("- [x] No remaining items identified.".to_string());
    } else {
        for item in &review.remaining_items {
            lines.push(format!("- [ ] {item}"));
        }
    }
    lines.extend([String::new(), "### Validation".to_string(), String::new()]);
    for item in &review.validation_completed {
        lines.push(format!("- [x] {item}"));
    }
    for item in &review.validation_remaining {
        lines.push(format!("- [ ] {item}"));
    }
    if review.validation_completed.is_empty() && review.validation_remaining.is_empty() {
        lines.push("- [ ] No explicit validation status recorded.".to_string());
    }
    lines.join("\n")
}

fn upsert_marked_section(contents: &str, marker: &str, body: &str) -> String {
    let start = format!("<!-- {marker}:start -->");
    let end = format!("<!-- {marker}:end -->");
    let replacement = format!("{start}\n{body}\n{end}");
    if let Some(start_index) = contents.find(&start)
        && let Some(end_index) = contents.find(&end)
    {
        let suffix_start = end_index + end.len();
        return format!(
            "{}{}{}",
            &contents[..start_index],
            replacement,
            &contents[suffix_start..]
        );
    }
    let mut updated = contents.trim_end().to_string();
    if !updated.is_empty() {
        updated.push_str("\n\n");
    }
    updated.push_str(&replacement);
    updated.push('\n');
    updated
}

fn build_worker_session(
    issue: &IssueSummary,
    phase: SessionPhase,
    summary: String,
    context: &WorkerSessionContext<'_>,
    turns: u32,
    session_id: Option<&str>,
    canonical: &CanonicalSessionData,
) -> super::AgentSession {
    let pid = match phase {
        SessionPhase::Completed | SessionPhase::Blocked => None,
        _ => context.pid.filter(|value| *value > 0),
    };
    super::AgentSession {
        issue_id: Some(issue.id.clone()),
        issue_identifier: issue.identifier.clone(),
        issue_title: issue.title.clone(),
        project_name: issue.project.as_ref().map(|project| project.name.clone()),
        team_key: issue.team.key.clone(),
        issue_url: issue.url.clone(),
        phase,
        summary,
        blocked: None,
        brief_path: Some(
            PlanningPaths::new(context.workspace_path)
                .agent_briefs_dir
                .join(format!("{}.md", issue.identifier))
                .display()
                .to_string(),
        ),
        backlog_issue_identifier: context
            .backlog_issue
            .map(|backlog_issue| backlog_issue.identifier.clone()),
        backlog_issue_title: context
            .backlog_issue
            .map(|backlog_issue| backlog_issue.title.clone()),
        backlog_path: context.backlog_issue.map(|backlog_issue| {
            PlanningPaths::new(context.workspace_path)
                .backlog_issue_dir(&backlog_issue.identifier)
                .display()
                .to_string()
        }),
        workspace_path: Some(context.workspace_path.display().to_string()),
        branch: context.branch.map(str::to_string),
        pull_request: context.pull_request.clone(),
        workpad_comment_id: Some(context.workpad_comment_id.to_string()),
        started_at_epoch_seconds: context.started_at_epoch_seconds,
        updated_at_epoch_seconds: now_epoch_seconds(),
        pid,
        session_id: session_id.map(str::to_string),
        latest_resume_handle: context.latest_resume_handle.clone(),
        context_budget_tokens: Some(context.context_budget_tokens),
        pending_linear_sync: context.pending_linear_sync.clone(),
        stale_worker_recovery_attempt_count: context.stale_worker_recovery_attempt_count,
        latest_stale_worker_failure: context.latest_stale_worker_failure.clone(),
        last_timeout: context.last_timeout.clone(),
        turns: Some(turns),
        tokens: canonical.tokens.clone(),
        turn_history: context.turn_history.clone(),
        canonical: canonical.clone(),
        log_path: Some(
            agent_log_path(
                context.source_root,
                context.project_selector,
                &issue.identifier,
            )
            .display()
            .to_string(),
        ),
        origin: context.origin,
    }
}

fn build_worker_blocked_session(
    issue: &IssueSummary,
    blocked: BlockedReason,
    summary: String,
    context: &WorkerSessionContext<'_>,
    turns: u32,
    session_id: Option<&str>,
    canonical: &CanonicalSessionData,
) -> super::AgentSession {
    let mut session = build_worker_session(
        issue,
        SessionPhase::Blocked,
        summary,
        context,
        turns,
        session_id,
        canonical,
    );
    session.blocked = Some(blocked);
    session
}

fn load_existing_session_tokens(
    root: &Path,
    project_selector: Option<&str>,
    issue_identifier: &str,
) -> Result<TokenUsage> {
    let store = super::store::ListenProjectStore::resolve(root, project_selector)?;
    let state = store.load_state()?;
    Ok(state
        .sessions
        .into_iter()
        .find(|session| session.issue_matches(issue_identifier))
        .map(|session| session.tokens)
        .unwrap_or_default())
}

fn load_existing_started_at_epoch_seconds(
    root: &Path,
    project_selector: Option<&str>,
    issue_identifier: &str,
) -> Result<u64> {
    let store = super::store::ListenProjectStore::resolve(root, project_selector)?;
    let state = store.load_state()?;
    Ok(state
        .sessions
        .into_iter()
        .find(|session| session.issue_matches(issue_identifier))
        .map(|session| {
            if session.started_at_epoch_seconds > 0 {
                session.started_at_epoch_seconds
            } else {
                session.updated_at_epoch_seconds
            }
        })
        .unwrap_or_else(now_epoch_seconds))
}

fn load_existing_stale_worker_recovery_attempt_count(
    root: &Path,
    project_selector: Option<&str>,
    issue_identifier: &str,
) -> Result<u32> {
    let store = super::store::ListenProjectStore::resolve(root, project_selector)?;
    let state = store.load_state()?;
    Ok(state
        .sessions
        .into_iter()
        .find(|session| session.issue_matches(issue_identifier))
        .map(|session| session.stale_worker_recovery_attempt_count)
        .unwrap_or(0))
}

fn load_existing_latest_stale_worker_failure(
    root: &Path,
    project_selector: Option<&str>,
    issue_identifier: &str,
) -> Result<Option<super::StaleWorkerFailure>> {
    let store = super::store::ListenProjectStore::resolve(root, project_selector)?;
    let state = store.load_state()?;
    Ok(state
        .sessions
        .into_iter()
        .find(|session| session.issue_matches(issue_identifier))
        .and_then(|session| session.latest_stale_worker_failure))
}

fn load_existing_turn_history(
    root: &Path,
    project_selector: Option<&str>,
    issue_identifier: &str,
) -> Result<Vec<TurnTokenSnapshot>> {
    let store = super::store::ListenProjectStore::resolve(root, project_selector)?;
    let state = store.load_state()?;
    Ok(state
        .sessions
        .into_iter()
        .find(|session| session.issue_matches(issue_identifier))
        .map(|session| session.turn_history)
        .unwrap_or_default())
}

fn load_existing_session_canonical(
    root: &Path,
    project_selector: Option<&str>,
    issue_identifier: &str,
) -> Result<CanonicalSessionData> {
    let store = super::store::ListenProjectStore::resolve(root, project_selector)?;
    let state = store.load_state()?;
    Ok(state
        .sessions
        .into_iter()
        .find(|session| session.issue_matches(issue_identifier))
        .map(|session| session.canonical)
        .unwrap_or_default())
}

fn load_existing_provider_session_id(
    root: &Path,
    project_selector: Option<&str>,
    issue_identifier: &str,
) -> Result<Option<String>> {
    let store = super::store::ListenProjectStore::resolve(root, project_selector)?;
    let state = store.load_state()?;
    Ok(state
        .sessions
        .into_iter()
        .find(|session| session.issue_matches(issue_identifier))
        .and_then(|session| session.session_id))
}

fn load_existing_latest_resume_handle(
    root: &Path,
    project_selector: Option<&str>,
    issue_identifier: &str,
) -> Result<Option<LatestResumeHandle>> {
    let store = super::store::ListenProjectStore::resolve(root, project_selector)?;
    let state = store.load_state()?;
    Ok(state
        .sessions
        .into_iter()
        .find(|session| session.issue_matches(issue_identifier))
        .and_then(|session| session.latest_resume_handle))
}

fn load_existing_pending_linear_sync(
    root: &Path,
    project_selector: Option<&str>,
    issue_identifier: &str,
) -> Result<Option<PendingLinearSync>> {
    let store = super::store::ListenProjectStore::resolve(root, project_selector)?;
    let state = store.load_state()?;
    Ok(state
        .sessions
        .into_iter()
        .find(|session| session.issue_matches(issue_identifier))
        .and_then(|session| session.pending_linear_sync))
}

fn load_existing_last_timeout(
    root: &Path,
    project_selector: Option<&str>,
    issue_identifier: &str,
) -> Result<Option<SessionTimeoutRecord>> {
    let store = super::store::ListenProjectStore::resolve(root, project_selector)?;
    let state = store.load_state()?;
    Ok(state
        .sessions
        .into_iter()
        .find(|session| session.issue_matches(issue_identifier))
        .and_then(|session| session.last_timeout))
}

fn load_existing_turn_count(
    root: &Path,
    project_selector: Option<&str>,
    issue_identifier: &str,
) -> Result<u32> {
    let store = super::store::ListenProjectStore::resolve(root, project_selector)?;
    let state = store.load_state()?;
    Ok(state
        .sessions
        .into_iter()
        .find(|session| session.issue_matches(issue_identifier))
        .and_then(|session| session.turns)
        .unwrap_or(0))
}

fn load_existing_session_origin(
    root: &Path,
    project_selector: Option<&str>,
    issue_identifier: &str,
) -> Result<super::state::SessionOrigin> {
    let store = super::store::ListenProjectStore::resolve(root, project_selector)?;
    let state = store.load_state()?;
    Ok(state
        .sessions
        .into_iter()
        .find(|session| session.issue_matches(issue_identifier))
        .map(|session| session.origin)
        .unwrap_or_default())
}

fn load_existing_pull_request(
    root: &Path,
    project_selector: Option<&str>,
    issue_identifier: &str,
) -> Result<PullRequestSummary> {
    let store = super::store::ListenProjectStore::resolve(root, project_selector)?;
    let state = store.load_state()?;
    Ok(state
        .sessions
        .into_iter()
        .find(|session| session.issue_matches(issue_identifier))
        .map(|session| session.pull_request)
        .unwrap_or_default())
}

fn load_existing_verification_summary(
    root: &Path,
    project_selector: Option<&str>,
    issue_identifier: &str,
) -> Result<Option<VerificationSummary>> {
    let store = super::store::ListenProjectStore::resolve(root, project_selector)?;
    Ok(store
        .load_verification_report(issue_identifier)?
        .map(|report| report.summary_snapshot()))
}

#[cfg(test)]
mod tests {
    use super::{
        ExecutionTurnPlan, LatestResumeHandle, ListenTurnContext, Path, ResumeProvider,
        ReviewReport, TurnExecutionResult, Value, WorkerSessionContext, build_worker_session,
        continuation_id_for_invocation, parse_claude_resume_handle, parse_codex_resume_handle,
        plan_execution_turn, query_codex_threads, read_codex_session_index, render_review_workpad,
        update_resume_handle_after_turn,
    };
    use crate::config::{AppConfig, PlanningMeta};
    use crate::linear::{IssueComment, IssueSummary, TeamRef};
    use crate::listen::state::ContextPressure;
    use crate::listen::verification::{
        VerificationRecipeStep, VerificationStatus, VerificationSummary,
    };
    use crate::listen::{
        CanonicalSessionData, PendingLinearSync, PullRequestSummary, SessionOrigin, SessionPhase,
        SessionTimeoutRecord, SessionTimeoutTermination, TokenUsage, TurnPromptMode,
        TurnTokenSnapshot,
    };
    use std::fs;
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;
    use tempfile::tempdir;

    fn issue() -> IssueSummary {
        IssueSummary {
            id: "issue-1".to_string(),
            identifier: "ENG-10181".to_string(),
            title: "Track listen tokens".to_string(),
            description: None,
            url: "https://linear.app/issues/ENG-10181".to_string(),
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
        }
    }

    fn test_issue(identifier: &str) -> IssueSummary {
        IssueSummary {
            id: format!("{identifier}-id"),
            identifier: identifier.to_string(),
            title: format!("{identifier} title"),
            description: None,
            url: format!("https://linear.app/issues/{identifier}"),
            priority: None,
            estimate: None,
            updated_at: "2026-03-19T00:00:00Z".to_string(),
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
        }
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
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

    fn listen_worker_args(
        source_root: &Path,
        workspace_path: &Path,
        context_budget_tokens: u64,
    ) -> crate::cli::ListenWorkerArgs {
        crate::cli::ListenWorkerArgs {
            source_root: source_root.to_path_buf(),
            project: None,
            workspace: workspace_path.to_path_buf(),
            issue: "ENG-10782".to_string(),
            workpad_comment_id: "comment-1".to_string(),
            backlog_issue: None,
            max_turns: 20,
            context_budget_tokens,
            api_key: None,
            api_url: None,
            profile: None,
            team: None,
            agent: None,
            model: None,
            reasoning: None,
        }
    }

    fn listen_turn_context<'a>(
        app_config: &'a AppConfig,
        planning_meta: &'a PlanningMeta,
        args: &'a crate::cli::ListenWorkerArgs,
        source_root: &'a Path,
        workspace_path: &'a Path,
    ) -> ListenTurnContext<'a> {
        ListenTurnContext {
            app_config,
            planning_meta,
            args,
            source_root,
            project_selector: None,
            workspace_path,
            workpad_comment_id: "comment-1",
            backlog_issue: None,
            max_turns: args.max_turns,
            context_budget_tokens: args.context_budget_tokens,
        }
    }

    fn worker_session_context<'a>(
        source_root: &'a Path,
        workspace_path: &'a Path,
        turn_history: Vec<TurnTokenSnapshot>,
    ) -> WorkerSessionContext<'a> {
        WorkerSessionContext {
            source_root,
            project_selector: None,
            workspace_path,
            branch: Some("eng-10782"),
            workpad_comment_id: "comment-1",
            backlog_issue: None,
            pid: None,
            latest_resume_handle: None,
            context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
            pending_linear_sync: None,
            started_at_epoch_seconds: 1,
            stale_worker_recovery_attempt_count: 0,
            latest_stale_worker_failure: None,
            last_timeout: None,
            turn_history,
            canonical: CanonicalSessionData::default(),
            pull_request: PullRequestSummary::default(),
            github_ci_summary: None,
            verification_summary: None,
            origin: SessionOrigin::Listen,
        }
    }

    fn set_env_var(key: &str, value: &str) {
        unsafe {
            std::env::set_var(key, value);
        }
    }

    fn restore_env_var(key: &str, value: Option<String>) {
        unsafe {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }

    #[test]
    fn worker_session_updates_keep_cumulative_tokens() {
        let issue = issue();
        let context = WorkerSessionContext {
            source_root: Path::new("/tmp/source"),
            project_selector: None,
            workspace_path: Path::new("/tmp/workspace"),
            branch: Some("eng-10181"),
            workpad_comment_id: "comment-1",
            backlog_issue: None,
            pid: Some(1234),
            latest_resume_handle: None,
            context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
            pending_linear_sync: None,
            started_at_epoch_seconds: 1,
            stale_worker_recovery_attempt_count: 0,
            latest_stale_worker_failure: None,
            last_timeout: None,
            turn_history: Vec::new(),
            canonical: CanonicalSessionData::default(),
            pull_request: PullRequestSummary::default(),
            github_ci_summary: None,
            verification_summary: None,
            origin: SessionOrigin::Listen,
        };
        let mut tokens = TokenUsage::default();

        let first = build_worker_session(
            &issue,
            SessionPhase::Running,
            "turn 1".to_string(),
            &context,
            0,
            Some("thread-1"),
            &CanonicalSessionData::default(),
        );
        assert_eq!(first.tokens.input, None);
        assert_eq!(first.tokens.output, None);

        tokens.accumulate(&TokenUsage {
            input: Some(120),
            output: None,
        });
        let second = build_worker_session(
            &issue,
            SessionPhase::Running,
            "turn 2".to_string(),
            &context,
            1,
            Some("thread-1"),
            &CanonicalSessionData {
                tokens: tokens.clone(),
                ..CanonicalSessionData::default()
            },
        );
        assert_eq!(second.tokens.input, Some(120));
        assert_eq!(second.tokens.output, None);
        assert_eq!(second.canonical.tokens.input, Some(120));
        assert_eq!(second.canonical.tokens.output, None);

        tokens.accumulate(&TokenUsage {
            input: None,
            output: Some(45),
        });
        let third = build_worker_session(
            &issue,
            SessionPhase::Completed,
            "done".to_string(),
            &context,
            2,
            Some("thread-1"),
            &CanonicalSessionData {
                tokens: tokens.clone(),
                ..CanonicalSessionData::default()
            },
        );
        assert_eq!(third.tokens.input, Some(120));
        assert_eq!(third.tokens.output, Some(45));
        assert_eq!(third.tokens.total(), Some(165));
        assert_eq!(third.canonical.tokens.input, Some(120));
        assert_eq!(third.canonical.tokens.output, Some(45));
        assert_eq!(third.canonical.tokens.total(), Some(165));
    }

    #[test]
    fn parses_claude_resume_handle_from_stream_json() {
        let value: Value = serde_json::from_str(
            r#"{"type":"system","subtype":"init","session_id":"513d2595-0968-4357-9339-489f1d21c1cf"}"#,
        )
        .expect("valid json");

        assert_eq!(
            parse_claude_resume_handle(&value),
            Some(LatestResumeHandle {
                provider: ResumeProvider::Claude,
                id: "513d2595-0968-4357-9339-489f1d21c1cf".to_string(),
            })
        );
    }

    #[test]
    fn parses_codex_resume_handle_from_thread_started_event() {
        let value: Value = serde_json::from_str(
            r#"{"type":"thread.started","thread_id":"019d0766-1ca5-70c3-ae80-afafe1fb7bff"}"#,
        )
        .expect("valid json");

        assert_eq!(
            parse_codex_resume_handle(&value),
            Some(LatestResumeHandle {
                provider: ResumeProvider::Codex,
                id: "019d0766-1ca5-70c3-ae80-afafe1fb7bff".to_string(),
            })
        );
    }

    #[test]
    fn continuation_id_for_invocation_reuses_matching_resume_handle() {
        let handle = LatestResumeHandle {
            provider: ResumeProvider::Codex,
            id: "thread-123".to_string(),
        };

        assert_eq!(
            continuation_id_for_invocation("codex", Some(&handle)),
            Some("thread-123".to_string())
        );
    }

    #[test]
    fn continuation_id_for_invocation_rejects_mismatched_provider() {
        let handle = LatestResumeHandle {
            provider: ResumeProvider::Claude,
            id: "session-123".to_string(),
        };

        assert_eq!(continuation_id_for_invocation("codex", Some(&handle)), None);
    }

    #[test]
    fn read_codex_session_index_filters_recent_entries() {
        let temp = tempdir().expect("tempdir should build");
        let codex_root = temp.path().join(".codex");
        fs::create_dir_all(&codex_root).expect("codex dir should exist");
        fs::write(
            codex_root.join("session_index.jsonl"),
            concat!(
                "{\"id\":\"recent\",\"updated_at\":\"2026-03-19T15:00:05Z\"}\n",
                "{\"id\":\"old\",\"updated_at\":\"2026-03-19T14:58:00Z\"}\n"
            ),
        )
        .expect("session index should write");

        let ids =
            read_codex_session_index(&codex_root, 1_773_932_400, 1_773_932_420).expect("index");

        assert_eq!(ids, vec!["recent".to_string()]);
    }

    #[test]
    fn query_codex_threads_returns_only_matching_rows() {
        let _guard = env_lock().lock().expect("env mutex should lock");
        let temp = tempdir().expect("tempdir should build");
        let workspace = temp.path().join("workspace");
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&workspace).expect("workspace dir should exist");
        fs::create_dir_all(&bin_dir).expect("bin dir should exist");
        let sqlite_path = bin_dir.join("sqlite3");
        fs::write(&sqlite_path, "#!/bin/sh\nprintf '%s' \"$SQLITE3_ROWS\"\n")
            .expect("sqlite stub should write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&sqlite_path)
                .expect("sqlite stub metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&sqlite_path, permissions).expect("sqlite stub permissions");
        }

        let original_path = std::env::var("PATH").ok();
        set_env_var(
            "PATH",
            &format!(
                "{}:{}",
                bin_dir.display(),
                original_path.clone().unwrap_or_default()
            ),
        );
        set_env_var("SQLITE3_ROWS", "thread-1|1773945466|1773945607\n");

        let init = std::process::Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(&workspace)
            .status()
            .expect("git init should run");
        assert!(init.success());
        let checkout = std::process::Command::new("git")
            .arg("-C")
            .arg(&workspace)
            .args(["checkout", "-b", "eng-10194"])
            .status()
            .expect("git checkout should run");
        assert!(checkout.success());

        let rows = query_codex_threads(
            Path::new("/tmp/fake-state.sqlite"),
            &workspace,
            &test_issue("ENG-10194"),
            1_773_945_460,
            1_773_945_610,
            &["thread-1".to_string()],
        )
        .expect("sqlite query should succeed");

        restore_env_var("PATH", original_path);
        restore_env_var("SQLITE3_ROWS", None);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "thread-1");
    }

    #[test]
    fn query_codex_threads_rejects_ambiguous_rows() {
        let _guard = env_lock().lock().expect("env mutex should lock");
        let temp = tempdir().expect("tempdir should build");
        let workspace = temp.path().join("workspace");
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&workspace).expect("workspace dir should exist");
        fs::create_dir_all(&bin_dir).expect("bin dir should exist");
        let sqlite_path = bin_dir.join("sqlite3");
        fs::write(&sqlite_path, "#!/bin/sh\nprintf '%s' \"$SQLITE3_ROWS\"\n")
            .expect("sqlite stub should write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&sqlite_path)
                .expect("sqlite stub metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&sqlite_path, permissions).expect("sqlite stub permissions");
        }

        let original_path = std::env::var("PATH").ok();
        set_env_var(
            "PATH",
            &format!(
                "{}:{}",
                bin_dir.display(),
                original_path.clone().unwrap_or_default()
            ),
        );
        set_env_var(
            "SQLITE3_ROWS",
            "thread-1|1773945466|1773945607\nthread-2|1773945468|1773945608\n",
        );

        let init = std::process::Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(&workspace)
            .status()
            .expect("git init should run");
        assert!(init.success());

        let rows = query_codex_threads(
            Path::new("/tmp/fake-state.sqlite"),
            &workspace,
            &test_issue("ENG-10194"),
            1_773_945_460,
            1_773_945_610,
            &[],
        )
        .expect("sqlite query should succeed");

        restore_env_var("PATH", original_path);
        restore_env_var("SQLITE3_ROWS", None);

        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn build_listen_run_args_uses_continuation_prompt_on_resume() {
        let temp = tempdir().expect("tempdir should build");
        let workspace = temp.path();
        fs::create_dir_all(workspace.join(".metastack")).expect("metastack dir should build");
        let source_root = temp.path();

        let issue = test_issue("MET-57");
        let app_config = crate::config::AppConfig::default();
        let planning_meta = crate::config::PlanningMeta::default();
        let args = crate::cli::ListenWorkerArgs {
            source_root: source_root.to_path_buf(),
            project: None,
            workspace: workspace.to_path_buf(),
            issue: "MET-57".to_string(),
            workpad_comment_id: "comment-1".to_string(),
            backlog_issue: None,
            max_turns: 20,
            context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
            api_key: None,
            api_url: None,
            profile: None,
            team: None,
            agent: None,
            model: None,
            reasoning: None,
        };
        let context = super::ListenTurnContext {
            app_config: &app_config,
            planning_meta: &planning_meta,
            args: &args,
            source_root,
            project_selector: None,
            workspace_path: workspace,
            workpad_comment_id: "comment-1",
            backlog_issue: None,
            max_turns: 20,
            context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
        };
        let plan =
            super::ExecutionTurnPlan::new(super::ContextPressure::Normal, 0, false, false, 2, 20);

        // Turn 2 with resume handle → continuation prompt, no instructions.
        let resumed = super::build_listen_run_args(&issue, 2, &context, plan, None, None, true)
            .expect("build_listen_run_args should succeed");
        assert!(
            resumed.prompt.contains("Continuation guidance"),
            "resume turn 2 should use continuation prompt"
        );
        assert!(
            resumed.instructions.is_none(),
            "resume turn 2 should omit instructions"
        );

        // Turn 2 without resume handle → full prompt with instructions.
        let cold = super::build_listen_run_args(&issue, 2, &context, plan, None, None, false)
            .expect("build_listen_run_args should succeed");
        assert!(
            cold.prompt.contains("Execution continuation"),
            "cold turn 2 should use compact continuation prompt"
        );
        assert!(
            cold.instructions.is_some(),
            "cold turn 2 should include instructions"
        );
    }

    #[test]
    fn build_listen_run_args_uses_full_prompt_on_turn_one_even_with_resume() {
        let temp = tempdir().expect("tempdir should build");
        let workspace = temp.path();
        fs::create_dir_all(workspace.join(".metastack")).expect("metastack dir should build");
        let source_root = temp.path();

        let issue = test_issue("MET-57");
        let app_config = crate::config::AppConfig::default();
        let planning_meta = crate::config::PlanningMeta::default();
        let args = crate::cli::ListenWorkerArgs {
            source_root: source_root.to_path_buf(),
            project: None,
            workspace: workspace.to_path_buf(),
            issue: "MET-57".to_string(),
            workpad_comment_id: "comment-1".to_string(),
            backlog_issue: None,
            max_turns: 20,
            context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
            api_key: None,
            api_url: None,
            profile: None,
            team: None,
            agent: None,
            model: None,
            reasoning: None,
        };
        let context = super::ListenTurnContext {
            app_config: &app_config,
            planning_meta: &planning_meta,
            args: &args,
            source_root,
            project_selector: None,
            workspace_path: workspace,
            workpad_comment_id: "comment-1",
            backlog_issue: None,
            max_turns: 20,
            context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
        };
        let plan =
            super::ExecutionTurnPlan::new(super::ContextPressure::Normal, 0, false, false, 1, 20);

        // Turn 1 with resume handle should still use full prompt (initial context load).
        let result = super::build_listen_run_args(&issue, 1, &context, plan, None, None, true)
            .expect("build_listen_run_args should succeed");
        assert!(
            result.prompt.contains("You are working on Linear ticket"),
            "turn 1 should always use full prompt"
        );
        assert!(
            result.instructions.is_some(),
            "turn 1 should always include instructions"
        );
    }

    #[test]
    fn build_listen_run_args_only_warns_for_elevated_pressure_on_continuations() {
        let temp = tempdir().expect("tempdir should build");
        let workspace = temp.path();
        fs::create_dir_all(workspace.join(".metastack")).expect("metastack dir should build");
        let source_root = temp.path();
        let app_config = AppConfig::default();
        let planning_meta = PlanningMeta::default();
        let args = listen_worker_args(
            source_root,
            workspace,
            crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
        );
        let context =
            listen_turn_context(&app_config, &planning_meta, &args, source_root, workspace);
        let plan = ExecutionTurnPlan::new(ContextPressure::Elevated, 140_000, false, false, 2, 20);
        let issue = test_issue("ENG-10782");

        let resumed = super::build_listen_run_args(&issue, 2, &context, plan, None, None, true)
            .expect("resumed run args should build");
        assert!(resumed.prompt.contains("Context pressure guidance:"));
        assert!(resumed.prompt.contains("Context pressure: elevated."));

        let cold = super::build_listen_run_args(&issue, 2, &context, plan, None, None, false)
            .expect("cold run args should build");
        assert!(!cold.prompt.contains("Context pressure guidance:"));
        assert!(!cold.prompt.contains("Context pressure: elevated."));
    }

    #[test]
    fn update_resume_handle_after_turn_keeps_handle_on_normal_turns() {
        let temp = tempdir().expect("tempdir should build");
        let mut session_context =
            worker_session_context(temp.path(), temp.path(), vec![turn_snapshot(1, Some(20))]);
        session_context.latest_resume_handle = Some(LatestResumeHandle {
            provider: ResumeProvider::Claude,
            id: "existing-handle".to_string(),
        });

        update_resume_handle_after_turn(
            &mut session_context,
            &TurnExecutionResult {
                latest_resume_handle: Some(LatestResumeHandle {
                    provider: ResumeProvider::Claude,
                    id: "new-handle".to_string(),
                }),
                ..TurnExecutionResult::default()
            },
            ExecutionTurnPlan::new(ContextPressure::Normal, 20, false, false, 2, 20),
            "ENG-10782",
            2,
        );

        assert_eq!(
            session_context
                .latest_resume_handle
                .as_ref()
                .map(|handle| handle.id.as_str()),
            Some("new-handle")
        );
    }

    #[test]
    fn update_resume_handle_after_turn_clears_handle_after_checkpoint_turn() {
        let temp = tempdir().expect("tempdir should build");
        let mut session_context =
            worker_session_context(temp.path(), temp.path(), vec![turn_snapshot(1, Some(90))]);

        update_resume_handle_after_turn(
            &mut session_context,
            &TurnExecutionResult {
                latest_resume_handle: Some(LatestResumeHandle {
                    provider: ResumeProvider::Claude,
                    id: "checkpoint-handle".to_string(),
                }),
                ..TurnExecutionResult::default()
            },
            ExecutionTurnPlan::new(ContextPressure::High, 90, true, false, 2, 20),
            "ENG-10782",
            2,
        );

        assert!(session_context.latest_resume_handle.is_none());
    }

    #[test]
    fn update_resume_handle_after_turn_preserves_handle_when_checkpoint_turn_times_out() {
        let temp = tempdir().expect("tempdir should build");
        let mut session_context =
            worker_session_context(temp.path(), temp.path(), vec![turn_snapshot(1, Some(96))]);

        update_resume_handle_after_turn(
            &mut session_context,
            &TurnExecutionResult {
                latest_resume_handle: Some(LatestResumeHandle {
                    provider: ResumeProvider::Claude,
                    id: "critical-timeout".to_string(),
                }),
                timeout: Some(SessionTimeoutRecord {
                    turn: 2,
                    elapsed_seconds: 1,
                    timeout_seconds: 1,
                    graceful_shutdown_seconds: 5,
                    pid: 12345,
                    termination: SessionTimeoutTermination::Sigterm,
                }),
                ..TurnExecutionResult::default()
            },
            ExecutionTurnPlan::new(ContextPressure::Critical, 96, true, true, 2, 20),
            "ENG-10782",
            2,
        );

        assert_eq!(
            session_context
                .latest_resume_handle
                .as_ref()
                .map(|handle| handle.id.as_str()),
            Some("critical-timeout")
        );
    }

    #[test]
    fn build_agent_instructions_executes_ticket_without_label_based_modes() {
        let temp = tempdir().expect("tempdir should build");
        let workspace = temp.path();
        let source_root = temp.path();
        fs::create_dir_all(workspace.join(".metastack/agents/briefs"))
            .expect("brief dir should build");
        fs::create_dir_all(workspace.join(".metastack")).expect("metastack dir should build");
        fs::write(workspace.join("AGENTS.md"), "legacy").expect("agents should write");

        let app_config = crate::config::AppConfig::default();
        let planning_meta = crate::config::PlanningMeta::default();
        let args = crate::cli::ListenWorkerArgs {
            source_root: source_root.to_path_buf(),
            project: None,
            workspace: workspace.to_path_buf(),
            issue: "MET-57".to_string(),
            workpad_comment_id: "comment-1".to_string(),
            backlog_issue: None,
            max_turns: 20,
            context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
            api_key: None,
            api_url: None,
            profile: None,
            team: None,
            agent: None,
            model: None,
            reasoning: None,
        };
        let issue = test_issue("MET-57");
        let context = super::ListenTurnContext {
            app_config: &app_config,
            planning_meta: &planning_meta,
            args: &args,
            source_root,
            project_selector: None,
            workspace_path: workspace,
            workpad_comment_id: "comment-1",
            backlog_issue: None,
            max_turns: 20,
            context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
        };
        let plan =
            super::ExecutionTurnPlan::new(super::ContextPressure::Normal, 0, false, false, 1, 20);

        let instructions = super::build_agent_instructions(&issue, 1, &context, plan)
            .expect("instructions should build");

        assert!(instructions.contains("Treat the Linear ticket title, description, labels, and attached instructions as the primary work contract."));
        assert!(instructions.contains("Execute the requested work directly"));
        assert!(instructions.contains("meaningful workspace updates"));
        assert!(instructions.contains("WORKFLOW.md"));
        assert!(!instructions.contains("refine the workpad plan and acceptance criteria"));
        assert!(!instructions.contains("plan/spec oriented"));
    }

    #[test]
    fn listen_review_prompt_calls_out_cross_actor_state_ownership_risks() {
        let temp = tempdir().expect("tempdir should build");
        let workspace = temp.path();
        let source_root = temp.path();
        fs::create_dir_all(workspace.join(".metastack")).expect("metastack dir should build");

        let app_config = crate::config::AppConfig::default();
        let planning_meta = crate::config::PlanningMeta::default();
        let args = crate::cli::ListenWorkerArgs {
            source_root: source_root.to_path_buf(),
            project: None,
            workspace: workspace.to_path_buf(),
            issue: "ENG-10749".to_string(),
            workpad_comment_id: "comment-1".to_string(),
            backlog_issue: None,
            max_turns: 20,
            context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
            api_key: None,
            api_url: None,
            profile: None,
            team: None,
            agent: None,
            model: None,
            reasoning: None,
        };
        let context = super::ListenTurnContext {
            app_config: &app_config,
            planning_meta: &planning_meta,
            args: &args,
            source_root,
            project_selector: None,
            workspace_path: workspace,
            workpad_comment_id: "comment-1",
            backlog_issue: None,
            max_turns: 20,
            context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
        };
        let mut issue = test_issue("ENG-10749");
        issue.description = Some(
            "## Acceptance Criteria\n- Preserve listen state.\n\n## Validation\n- cargo test\n"
                .to_string(),
        );
        let review = super::ReviewReport {
            summary: "Review summary".to_string(),
            complete: true,
            completed_items: vec!["Preserved listen state.".to_string()],
            remaining_items: Vec::new(),
            validation_completed: vec!["cargo test".to_string()],
            validation_remaining: Vec::new(),
            risks: Vec::new(),
            notes: Vec::new(),
        };

        let review_instructions = super::build_review_instructions(&context);
        let final_instructions = super::build_final_review_instructions(&context);
        let review_prompt = super::render_review_prompt(&issue, 2, &context);
        let final_prompt = super::render_final_review_prompt(&issue, 2, &context, &review);

        for rendered in [
            review_instructions.as_str(),
            final_instructions.as_str(),
            review_prompt.as_str(),
            final_prompt.as_str(),
        ] {
            assert!(rendered.contains("cross-actor state ownership"));
            assert!(rendered.contains("spawn-before-persist"));
            assert!(rendered.contains("whole-state rewrites"));
            assert!(rendered.contains("blocked metadata"));
            assert!(rendered.contains("canonical metadata"));
            assert!(rendered.contains("`turn_history`"));
            assert!(rendered.contains("`latest_resume_handle`"));
            assert!(rendered.contains("src/listen/mod.rs"));
            assert!(rendered.contains("src/listen/store.rs"));
            assert!(rendered.contains("src/listen/worker.rs"));
        }
    }

    #[test]
    fn parse_agent_json_accepts_fenced_payloads() {
        let parsed: super::ReviewReport = super::parse_agent_json(
            "```json\n{\"summary\":\"ok\",\"complete\":false,\"remaining_items\":[\"finish docs\"]}\n```",
            "review",
        )
        .expect("review json should parse");

        assert_eq!(parsed.summary, "ok");
        assert_eq!(parsed.remaining_items, vec!["finish docs"]);
    }

    #[test]
    fn render_execution_delta_prompt_includes_verification_summary_and_remediation() {
        let plan =
            super::ExecutionTurnPlan::new(super::ContextPressure::Normal, 0, false, false, 2, 4);
        let prompt = super::render_execution_delta_prompt(
            &test_issue("MET-57"),
            2,
            4,
            None,
            Some(&VerificationSummary {
                status: VerificationStatus::Failed,
                summary: "Verification failed on the sampled input.".to_string(),
                criteria_total: 1,
                criteria_failed: 1,
                e2e_status: VerificationStatus::Passed,
                battle_test_status: VerificationStatus::Failed,
                remediation: vec![
                    "Repair the verifier finding.".to_string(),
                    "Re-run the sampled battle input.".to_string(),
                ],
            }),
            false,
            plan,
            "",
        );

        assert!(prompt.contains("Latest verification:"));
        assert!(prompt.contains("Verification failed on the sampled input."));
        assert!(prompt.contains("Repair the verifier finding."));
        assert!(prompt.contains("Re-run the sampled battle input."));
    }

    #[test]
    fn plan_execution_turn_uses_pending_workpad_checkpoint_to_skip_duplicate_checkpoint() {
        let temp = tempdir().expect("tempdir should build");
        let workspace = temp.path();
        let source_root = temp.path();
        let app_config = AppConfig::default();
        let planning_meta = PlanningMeta::default();
        let args = listen_worker_args(source_root, workspace, 100);
        let context =
            listen_turn_context(&app_config, &planning_meta, &args, source_root, workspace);
        let mut session_context =
            worker_session_context(source_root, workspace, vec![turn_snapshot(1, Some(96))]);
        session_context.pending_linear_sync = Some(PendingLinearSync {
            workpad_body: Some(
                "## Codex Workpad\n\n### Review Notes\n\n#### Context Checkpoint\n\n- already captured\n"
                    .to_string(),
            ),
            ..PendingLinearSync::default()
        });

        let plan = plan_execution_turn(&context, &test_issue("ENG-10782"), &session_context, 2);

        assert_eq!(plan.pressure, ContextPressure::Critical);
        assert!(!plan.checkpoint_turn);
        assert!(plan.last_execution_turn);
    }

    #[test]
    fn render_review_workpad_preserves_existing_context_checkpoint() {
        let temp = tempdir().expect("tempdir should build");
        let workspace = temp.path();
        let source_root = temp.path();
        let app_config = AppConfig::default();
        let planning_meta = PlanningMeta::default();
        let args = listen_worker_args(
            source_root,
            workspace,
            crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
        );
        let context =
            listen_turn_context(&app_config, &planning_meta, &args, source_root, workspace);
        let mut issue = test_issue("ENG-10782");
        issue.comments = vec![IssueComment {
            id: "comment-1".to_string(),
            body: "## Codex Workpad\n\n### Review Notes\n\n- Manual note\n\n#### Context Checkpoint\n\n- Pressure: high\n"
                .to_string(),
            created_at: None,
            user_name: None,
            resolved_at: None,
        }];
        let session_context = worker_session_context(source_root, workspace, Vec::new());
        let review = ReviewReport {
            summary: "Review complete".to_string(),
            complete: false,
            ..ReviewReport::default()
        };

        let body = render_review_workpad(&issue, &context, &review, &session_context, None);

        assert!(body.contains("#### Context Checkpoint"));
        assert!(body.contains("- Pressure: high"));
        assert!(body.contains("- Manual note"));
    }

    #[test]
    fn upsert_marked_section_replaces_existing_managed_block() {
        let original = "\
# Title

<!-- metastack-listen-progress:start -->
old
<!-- metastack-listen-progress:end -->
";
        let updated =
            super::upsert_marked_section(original, "metastack-listen-progress", "new body");

        assert!(updated.contains("new body"));
        assert!(!updated.contains("\nold\n"));
    }

    #[test]
    fn parse_claude_resume_handle_from_array_wrapped_stream_json() {
        // Claude --output-format=stream-json wraps each event in an array
        let line = r#"[{"type":"system","subtype":"init","session_id":"22ca497e-d7da-4118-9433-1902769c6737","tools":["Bash"]}]"#;
        let handle = super::parse_resume_handle_line("claude", line.as_bytes());
        assert!(
            handle.is_some(),
            "should parse session_id from array-wrapped JSON"
        );
        let handle = handle.unwrap();
        assert_eq!(handle.id, "22ca497e-d7da-4118-9433-1902769c6737");
        assert_eq!(handle.provider, super::super::state::ResumeProvider::Claude);
    }

    #[test]
    fn parse_claude_resume_handle_from_plain_object() {
        // Also works with unwrapped objects (e.g. --output-format=json)
        let line = r#"{"type":"result","session_id":"abc-123"}"#;
        let handle = super::parse_resume_handle_line("claude", line.as_bytes());
        assert!(
            handle.is_some(),
            "should parse session_id from plain JSON object"
        );
        assert_eq!(handle.unwrap().id, "abc-123");
    }

    #[test]
    fn heuristic_review_requires_failed_verification_to_complete_without_backlog_signal() {
        let temp = tempdir().expect("tempdir should build");
        let workspace = temp.path();
        fs::create_dir_all(workspace.join(".metastack")).expect("metastack dir should build");
        let source_root = temp.path();
        let app_config = AppConfig::default();
        let planning_meta = PlanningMeta::default();
        let args = crate::cli::ListenWorkerArgs {
            source_root: source_root.to_path_buf(),
            project: None,
            workspace: workspace.to_path_buf(),
            issue: "MET-57".to_string(),
            workpad_comment_id: "comment-1".to_string(),
            backlog_issue: None,
            max_turns: 20,
            context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
            api_key: None,
            api_url: None,
            profile: None,
            team: None,
            agent: None,
            model: None,
            reasoning: None,
        };
        let context = super::ListenTurnContext {
            app_config: &app_config,
            planning_meta: &planning_meta,
            args: &args,
            source_root,
            project_selector: None,
            workspace_path: workspace,
            workpad_comment_id: "comment-1",
            backlog_issue: None,
            max_turns: 20,
            context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
        };
        let progress = super::super::TurnProgress {
            planning_entries: Vec::new(),
            implementation_entries: vec!["src/lib.rs".to_string()],
        };

        let report = super::heuristic_review_report(
            &test_issue("MET-57"),
            &context,
            true,
            &progress,
            true,
            None,
            None,
        )
        .expect("heuristic review should succeed");

        assert!(!report.complete);
        assert!(
            report.remaining_items.is_empty(),
            "no acceptance criteria means the heuristic should wait for a stronger signal"
        );
    }

    #[test]
    fn heuristic_review_allows_failed_verification_retry_without_backlog_signal() {
        let temp = tempdir().expect("tempdir should build");
        let workspace = temp.path();
        fs::create_dir_all(workspace.join(".metastack")).expect("metastack dir should build");
        let source_root = temp.path();
        let app_config = AppConfig::default();
        let planning_meta = PlanningMeta::default();
        let args = crate::cli::ListenWorkerArgs {
            source_root: source_root.to_path_buf(),
            project: None,
            workspace: workspace.to_path_buf(),
            issue: "MET-57".to_string(),
            workpad_comment_id: "comment-1".to_string(),
            backlog_issue: None,
            max_turns: 20,
            context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
            api_key: None,
            api_url: None,
            profile: None,
            team: None,
            agent: None,
            model: None,
            reasoning: None,
        };
        let context = super::ListenTurnContext {
            app_config: &app_config,
            planning_meta: &planning_meta,
            args: &args,
            source_root,
            project_selector: None,
            workspace_path: workspace,
            workpad_comment_id: "comment-1",
            backlog_issue: None,
            max_turns: 20,
            context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
        };
        let progress = super::super::TurnProgress {
            planning_entries: Vec::new(),
            implementation_entries: vec!["src/lib.rs".to_string()],
        };

        let report = super::heuristic_review_report(
            &test_issue("MET-57"),
            &context,
            true,
            &progress,
            true,
            None,
            Some(&VerificationSummary {
                status: VerificationStatus::Failed,
                summary: "Previous verification failed.".to_string(),
                ..VerificationSummary::default()
            }),
        )
        .expect("heuristic review should succeed");

        assert!(report.complete);
        assert_eq!(
            report.summary,
            "Heuristic review believes the ticket work is complete."
        );
    }

    #[test]
    fn heuristic_review_allows_validation_retry_without_backlog_signal() {
        let temp = tempdir().expect("tempdir should build");
        let workspace = temp.path();
        fs::create_dir_all(workspace.join(".metastack")).expect("metastack dir should build");
        let source_root = temp.path();
        let app_config = AppConfig::default();
        let planning_meta = PlanningMeta::default();
        let args = crate::cli::ListenWorkerArgs {
            source_root: source_root.to_path_buf(),
            project: None,
            workspace: workspace.to_path_buf(),
            issue: "MET-57".to_string(),
            workpad_comment_id: "comment-1".to_string(),
            backlog_issue: None,
            max_turns: 20,
            context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
            api_key: None,
            api_url: None,
            profile: None,
            team: None,
            agent: None,
            model: None,
            reasoning: None,
        };
        let context = super::ListenTurnContext {
            app_config: &app_config,
            planning_meta: &planning_meta,
            args: &args,
            source_root,
            project_selector: None,
            workspace_path: workspace,
            workpad_comment_id: "comment-1",
            backlog_issue: None,
            max_turns: 20,
            context_budget_tokens: crate::config::DEFAULT_LISTEN_CONTEXT_BUDGET_TOKENS,
        };
        let progress = super::super::TurnProgress {
            planning_entries: Vec::new(),
            implementation_entries: vec!["src/lib.rs".to_string()],
        };
        let report = super::heuristic_review_report(
            &test_issue("MET-57"),
            &context,
            true,
            &progress,
            true,
            Some(&super::ReviewReport {
                validation_remaining: vec![
                    "Local validation must pass before ready promotion.".to_string(),
                ],
                ..super::ReviewReport::default()
            }),
            Some(&VerificationSummary {
                status: VerificationStatus::Passed,
                summary: "Verification already passed.".to_string(),
                ..VerificationSummary::default()
            }),
        )
        .expect("heuristic review should succeed");

        assert!(report.complete);
    }

    #[test]
    fn e2e_recipe_step_times_out_cleanly() {
        let temp = tempdir().expect("tempdir should build");
        let report = super::run_e2e_recipe_step_with_timeout(
            temp.path(),
            &VerificationRecipeStep {
                name: "sleepy".to_string(),
                command: vec!["sh".to_string(), "-c".to_string(), "sleep 1".to_string()],
                ..VerificationRecipeStep::default()
            },
            Duration::from_millis(50),
        )
        .expect("timed e2e step should return a report");

        assert_eq!(report.status, VerificationStatus::Failed);
        assert_eq!(report.timeout_seconds, Some(0));
        assert!(report.elapsed_seconds.is_some());
        assert!(report.timed_out);
        assert!(
            report
                .assertions
                .iter()
                .any(|assertion| assertion.contains("timed out")),
            "assertions={:?}",
            report.assertions
        );
    }
}

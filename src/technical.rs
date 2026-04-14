use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
#[cfg(test)]
use ratatui::backend::TestBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

use crate::agents::{AgentContinuation, run_agent_capture_with_continuation};
use crate::backlog::{
    BacklogIssueMetadata, INDEX_FILE_NAME, ManagedFileRecord, RenderedTemplateFile,
    TemplateContext, ensure_no_unresolved_placeholders, render_template_files, save_issue_metadata,
    write_rendered_backlog_item,
};
use crate::backlog_defaults::{
    TechnicalTicketResolutionInput, TicketOptionOverrides, load_remembered_backlog_selection,
    resolve_technical_ticket_defaults, save_remembered_backlog_selection,
};
use crate::branding;
use crate::cli::{RunAgentArgs, TechnicalArgs};
use crate::codebase_context::{
    CodebaseContextSection, MissingCodebaseContextHint, load_codebase_context_bundle,
};
use crate::config::{AGENT_ROUTE_BACKLOG_TECH, AppConfig, load_required_planning_meta};
use crate::context::load_workflow_contract;
use crate::fs::{canonicalize_existing_dir, display_path};
use crate::linear::browser::{
    IssueSearchResult, render_issue_preview, render_issue_row, search_issues,
};
use crate::linear::{
    IssueCreateSpec, IssueListFilters, IssueSummary, PreparedIssueContext, TicketDiscussionBudgets,
    materialize_issue_context, prepare_issue_context, render_ticket_image_summary,
};
use crate::output::{MachineIssueSummary, render_json_success};
use crate::progress::{
    LoadingPanelData, SPINNER_FRAMES, agent_loading_status_line, render_loading_panel,
};
use crate::scaffold::{ensure_backlog_templates, ensure_planning_layout};
use crate::sync_command::run_sync_push_for_issue;
use crate::tui::copy::{
    CopyPayload, CopyUiState, copy_overlay_viewport, field_copy_help, pane_copy_help,
};
use crate::tui::fields::{InputFieldState, MultiSelectFieldState};
use crate::tui::keybindings::{is_copy_key, is_mouse_toggle_key, top_level_cancel};
use crate::tui::markdown::render_markdown;
use crate::tui::scroll::{
    ScrollState, plain_text, scrollable_content_paragraph, scrollable_paragraph_with_block,
    wrapped_rows,
};
use crate::{LinearCommandContext, load_linear_command_context};

const ISSUE_PICKER_LIMIT: usize = 250;
const SKIPPED_TECHNICAL_FOLLOW_UP_LABEL: &str = "Skipped intentionally.";

#[derive(Debug, Clone, Deserialize, Serialize)]
struct TechnicalBacklogFile {
    path: String,
    contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TechnicalFollowUpResponse {
    question: String,
    answer: String,
    skipped: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FollowUpAnswerState {
    Pending,
    Answered,
    Skipped,
}

#[derive(Debug, Clone)]
struct QuestionAnswer {
    question: String,
    answer: InputFieldState,
    state: FollowUpAnswerState,
}

#[derive(Debug, Clone)]
struct TechnicalGeneratedBacklog {
    parent: IssueSummary,
    child_title: String,
    prepared_context: PreparedIssueContext,
    files: Vec<RenderedTemplateFile>,
}

#[derive(Debug, Clone)]
struct TechnicalWorkflowState {
    parent: IssueSummary,
    child_title: String,
    selected_acceptance_criteria: Vec<String>,
    prepared_context: PreparedIssueContext,
    template_files: Vec<RenderedTemplateFile>,
    backlog_slug: String,
    today: String,
    follow_ups: Vec<TechnicalFollowUpResponse>,
    questions_asked: usize,
    refinement_history: Vec<String>,
    files: Vec<RenderedTemplateFile>,
    revision: usize,
}

impl TechnicalWorkflowState {
    fn to_generated_backlog(&self) -> Result<TechnicalGeneratedBacklog> {
        if self.files.is_empty() {
            bail!("technical backlog draft is empty");
        }
        Ok(TechnicalGeneratedBacklog {
            parent: self.parent.clone(),
            child_title: self.child_title.clone(),
            prepared_context: self.prepared_context.clone(),
            files: self.files.clone(),
        })
    }
}

#[derive(Debug, Clone)]
struct IssuePickerApp {
    query: InputFieldState,
    issues: Vec<IssueSummary>,
    selected: usize,
    focus: IssuePickerFocus,
    preview_scroll: ScrollState,
    error: Option<String>,
    sticky_error: bool,
}

#[derive(Debug, Clone)]
struct TechnicalQuestionsApp {
    workflow: TechnicalWorkflowState,
    questions: Vec<QuestionAnswer>,
    selected: usize,
    error: Option<String>,
    sticky_error: bool,
}

#[derive(Debug, Clone)]
struct TechnicalReviewApp {
    workflow: TechnicalWorkflowState,
    selected_file: usize,
    focus: TechnicalReviewFocus,
    preview_scroll: ScrollState,
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct TechnicalReviewRefinementApp {
    review: TechnicalReviewApp,
    addendum: InputFieldState,
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct AcceptanceCriteriaApp {
    parent: IssueSummary,
    criteria: MultiSelectFieldState,
    error: Option<String>,
    sticky_error: bool,
}

#[derive(Debug, Clone)]
struct LoadingApp {
    message: String,
    detail: String,
    spinner_index: usize,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
enum TechnicalStage {
    PickIssue(IssuePickerApp),
    SelectCriteria(AcceptanceCriteriaApp),
    Questions(TechnicalQuestionsApp),
    Refinement(TechnicalReviewRefinementApp),
    Loading(LoadingApp),
    Review(TechnicalReviewApp),
}

#[derive(Debug, Clone, Default)]
struct TechnicalAgentOverrides {
    agent: Option<String>,
    model: Option<String>,
    reasoning: Option<String>,
}

struct TechnicalSessionApp {
    stage: TechnicalStage,
    copy: CopyUiState,
    agent_overrides: TechnicalAgentOverrides,
    continuation: Option<AgentContinuation>,
    question_limit: usize,
    refinement_round_limit: usize,
    pending: Option<PendingTechnicalJob>,
}

struct PendingTechnicalJob {
    receiver: Receiver<TechnicalWorkerReport>,
    previous_stage: Option<TechnicalStage>,
}

struct TechnicalWorkerReport {
    continuation: Option<AgentContinuation>,
    outcome: Result<TechnicalWorkerOutcome>,
}

enum TechnicalWorkerOutcome {
    Questions(TechnicalQuestionsApp),
    Review(TechnicalReviewApp),
}

#[allow(clippy::large_enum_variant)]
enum TechnicalAction {
    None,
    SelectIssue(IssueSummary),
    Generate(TechnicalGenerationRequest),
    ContinueWithAnswers {
        workflow: TechnicalWorkflowState,
        follow_ups: Vec<TechnicalFollowUpResponse>,
    },
    OpenRefinement {
        review: TechnicalReviewApp,
    },
    Refine {
        review: TechnicalReviewApp,
        addendum: String,
    },
    Confirm(TechnicalGeneratedBacklog),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IssuePickerFocus {
    List,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TechnicalReviewFocus {
    Files,
    Preview,
}

#[derive(Debug, Clone)]
struct TechnicalGenerationRequest {
    parent: IssueSummary,
    selected_acceptance_criteria: Vec<String>,
    discussion_budgets: TicketDiscussionBudgets,
}

#[allow(clippy::large_enum_variant)]
enum InteractiveTechnicalExit {
    Cancelled,
    Confirmed(TechnicalGeneratedBacklog),
}

fn clear_error(error: &mut Option<String>, sticky_error: &mut bool) {
    *error = None;
    *sticky_error = false;
}

fn clear_error_for_navigation(error: &mut Option<String>, sticky_error: &bool) {
    if !*sticky_error {
        *error = None;
    }
}

fn set_transient_error(error: &mut Option<String>, sticky_error: &mut bool, message: String) {
    *error = Some(message);
    *sticky_error = false;
}

fn set_sticky_error(error: &mut Option<String>, sticky_error: &mut bool, message: String) {
    *error = Some(message);
    *sticky_error = true;
}

fn input_key_clears_sticky_error(key: crossterm::event::KeyEvent) -> bool {
    matches!(
        key.code,
        KeyCode::Backspace | KeyCode::Delete
            | KeyCode::Char('u') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
    ) || matches!(
        key.code,
        KeyCode::Char(_) if !key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
    ) || matches!(
        key.code,
        KeyCode::Enter if key
            .modifiers
            .contains(crossterm::event::KeyModifiers::SHIFT)
    )
}

#[derive(Debug, Clone)]
pub(crate) struct TechnicalReport {
    child: Option<IssueSummary>,
    parent: Option<IssueSummary>,
    backlog_path: Option<String>,
    cancelled: bool,
}

impl TechnicalReport {
    pub(crate) fn render(&self) -> String {
        if self.cancelled {
            return "Technical generation cancelled.".to_string();
        }

        match (&self.child, &self.parent, self.backlog_path.as_deref()) {
            (Some(child), Some(parent), Some(backlog_path)) => format!(
                "Created technical sub-issue {} under {} at {}.",
                child.identifier, parent.identifier, backlog_path,
            ),
            _ => "Technical generation completed.".to_string(),
        }
    }

    /// Render the technical-generation result in the standard machine-readable success envelope.
    pub(crate) fn render_json(&self) -> Result<String> {
        #[derive(Serialize)]
        struct TechnicalResult {
            #[serde(default, skip_serializing_if = "Option::is_none")]
            child_issue: Option<MachineIssueSummary>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            parent_issue: Option<MachineIssueSummary>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            backlog_path: Option<String>,
            cancelled: bool,
        }

        render_json_success(
            "backlog.tech",
            &TechnicalResult {
                child_issue: self.child.as_ref().map(MachineIssueSummary::from),
                parent_issue: self.parent.as_ref().map(MachineIssueSummary::from),
                backlog_path: self.backlog_path.clone(),
                cancelled: self.cancelled,
            },
        )
    }
}

/// Create a technical child issue, materialize its local backlog packet, and sync attachments.
///
/// Returns an error when planning metadata is missing, the parent issue cannot be loaded,
/// backlog generation fails, the Linear child issue cannot be created, or the initial child sync
/// push cannot publish the generated managed files.
pub async fn run_technical(args: &TechnicalArgs) -> Result<TechnicalReport> {
    let root = canonicalize_existing_dir(&args.client.root)?;
    let app_config = AppConfig::load()?;
    let planning_meta = load_required_planning_meta(&root, "technical")?;
    let discussion_budgets = resolve_ticket_discussion_budgets(&planning_meta);
    let question_limit = planning_meta.effective_technical_follow_up_question_limit(&app_config);
    let refinement_round_limit =
        planning_meta.effective_technical_refinement_round_limit(&app_config);
    ensure_planning_layout(&root, false)?;
    ensure_backlog_templates(&root, false)?;
    let LinearCommandContext {
        service,
        default_team,
        default_project_id,
        ..
    } = load_linear_command_context(&args.client, None)?;
    let can_launch_tui = io::stdin().is_terminal() && io::stdout().is_terminal();
    let run_non_interactive = args.no_interactive || !can_launch_tui;
    let remembered_selection = if run_non_interactive {
        load_remembered_backlog_selection(&root)?
    } else {
        Default::default()
    };
    let agent_overrides = TechnicalAgentOverrides {
        agent: args.agent.clone(),
        model: args.model.clone(),
        reasoning: args.reasoning.clone(),
    };

    let generated = if !run_non_interactive {
        let initial_parent = match args.issue.as_ref() {
            Some(issue) => Some(service.load_issue(issue).await?),
            None => None,
        };
        let available_issues = if initial_parent.is_none() {
            service
                .list_issues(IssueListFilters {
                    team: default_team.clone(),
                    project_id: default_project_id.clone(),
                    limit: ISSUE_PICKER_LIMIT,
                    ..IssueListFilters::default()
                })
                .await?
        } else {
            Vec::new()
        };

        match run_interactive_technical_session(
            &root,
            initial_parent,
            available_issues,
            discussion_budgets,
            question_limit,
            refinement_round_limit,
            agent_overrides.clone(),
        )? {
            InteractiveTechnicalExit::Cancelled => {
                return Ok(TechnicalReport {
                    child: None,
                    parent: None,
                    backlog_path: None,
                    cancelled: true,
                });
            }
            InteractiveTechnicalExit::Confirmed(generated) => generated,
        }
    } else {
        let issue = args.issue.as_ref().ok_or_else(|| {
            anyhow!(
                "`{} backlog tech` requires an issue identifier when running without a TTY",
                branding::COMMAND_NAME
            )
        })?;
        let parent = service.load_issue(issue).await?;
        let selected_acceptance_criteria =
            extract_acceptance_criteria(parent.description.as_deref());
        run_non_interactive_technical_generation(
            &root,
            parent,
            selected_acceptance_criteria,
            discussion_budgets,
            question_limit,
            &args.answers,
            &agent_overrides,
        )?
    };

    let resolved_defaults = resolve_technical_ticket_defaults(
        &app_config,
        &planning_meta,
        &remembered_selection,
        &TechnicalTicketResolutionInput {
            zero_prompt: run_non_interactive,
            overrides: TicketOptionOverrides {
                state: args.state.clone(),
                priority: args.priority,
                labels: args.labels.clone(),
                assignee: args.assignee.clone(),
            },
            built_in_label: planning_meta.effective_technical_label(&app_config),
        },
        &generated.parent,
    );
    let assignee_id = service
        .resolve_assignee_id(resolved_defaults.assignee.as_deref())
        .await?;
    let child = service
        .create_issue(IssueCreateSpec {
            team: resolved_defaults.team.clone(),
            title: generated.child_title.clone(),
            description: Some(rendered_index_contents(&generated.files)?),
            project: resolved_defaults.project.clone(),
            project_id: resolved_defaults.project_id.clone(),
            project_milestone_id: None,
            parent_id: Some(generated.parent.id.clone()),
            state: resolved_defaults.state.clone(),
            priority: resolved_defaults.priority,
            assignee_id,
            labels: resolved_defaults.labels.clone(),
        })
        .await?;
    if let Err(error) = save_remembered_backlog_selection(&root, &child) {
        eprintln!("warning: failed to persist remembered backlog defaults: {error}");
    }

    let issue_dir = write_rendered_backlog_item(&root, &child.identifier, &generated.files)?;
    let download_failures =
        materialize_issue_context(&service, &issue_dir, &generated.prepared_context).await?;
    log_ticket_image_download_failures(&child.identifier, &download_failures);
    save_issue_metadata(
        &issue_dir,
        &BacklogIssueMetadata {
            issue_id: child.id.clone(),
            identifier: child.identifier.clone(),
            title: child.title.clone(),
            url: child.url.clone(),
            team_key: child.team.key.clone(),
            project_id: child.project.as_ref().map(|project| project.id.clone()),
            project_name: child.project.as_ref().map(|project| project.name.clone()),
            parent_id: Some(generated.parent.id.clone()),
            parent_identifier: Some(generated.parent.identifier.clone()),
            local_hash: None,
            remote_hash: None,
            last_sync_at: None,
            last_pulled_comment_ids: Vec::new(),
            managed_files: Vec::<ManagedFileRecord>::new(),
        },
    )?;

    run_sync_push_for_issue(&root, &service, &child, &issue_dir, args.no_interactive).await?;

    Ok(TechnicalReport {
        child: Some(child),
        parent: Some(generated.parent),
        backlog_path: Some(display_path(&issue_dir, &root)),
        cancelled: false,
    })
}

fn run_non_interactive_technical_generation(
    root: &Path,
    parent: IssueSummary,
    selected_acceptance_criteria: Vec<String>,
    discussion_budgets: TicketDiscussionBudgets,
    question_limit: usize,
    answers: &[String],
    overrides: &TechnicalAgentOverrides,
) -> Result<TechnicalGeneratedBacklog> {
    let mut workflow = build_technical_workflow_state(
        root,
        &parent,
        &selected_acceptance_criteria,
        discussion_budgets,
    )?;
    let mut continuation = None;

    loop {
        let remaining_questions = question_limit.saturating_sub(workflow.questions_asked);
        let outcome = if workflow.follow_ups.is_empty() {
            generate_technical_route_outcome(
                root,
                workflow,
                TechnicalPromptKind::Initial,
                remaining_questions,
                overrides,
                &mut continuation,
            )?
        } else {
            generate_technical_route_outcome(
                root,
                workflow,
                TechnicalPromptKind::FollowUp,
                remaining_questions,
                overrides,
                &mut continuation,
            )?
        };

        match outcome {
            TechnicalRouteOutcome::Questions {
                workflow: mut next_workflow,
                questions,
            } => {
                let required_answers = next_workflow.questions_asked;
                validate_non_interactive_answer_floor(answers.len(), required_answers)?;
                let provided = &answers[next_workflow.follow_ups.len()..required_answers];
                next_workflow.follow_ups.extend(
                    questions.into_iter().zip(provided.iter().cloned()).map(
                        |(question, answer)| TechnicalFollowUpResponse {
                            question,
                            answer,
                            skipped: false,
                        },
                    ),
                );
                workflow = next_workflow;
            }
            TechnicalRouteOutcome::Draft(workflow_with_draft) => {
                validate_non_interactive_answer_count(
                    answers.len(),
                    workflow_with_draft.questions_asked,
                )?;
                return workflow_with_draft.to_generated_backlog();
            }
        }
    }
}

fn validate_non_interactive_answer_floor(provided: usize, required: usize) -> Result<()> {
    if provided >= required {
        return Ok(());
    }

    bail!(
        "technical agent requested {required} follow-up question(s) so far; pass at least {required} `--answer` value(s)"
    );
}

fn validate_non_interactive_answer_count(provided: usize, required: usize) -> Result<()> {
    if provided == required {
        return Ok(());
    }

    if required == 0 {
        bail!(
            "technical agent requested no follow-up questions; remove the provided `--answer` values"
        );
    }

    bail!(
        "technical agent requested {required} follow-up question(s); pass exactly {required} `--answer` value(s)"
    );
}

fn run_interactive_technical_session(
    root: &Path,
    initial_parent: Option<IssueSummary>,
    issues: Vec<IssueSummary>,
    discussion_budgets: TicketDiscussionBudgets,
    question_limit: usize,
    refinement_round_limit: usize,
    agent_overrides: TechnicalAgentOverrides,
) -> Result<InteractiveTechnicalExit> {
    let mut app = if let Some(parent) = initial_parent {
        let criteria = extract_acceptance_criteria(parent.description.as_deref());
        if criteria.is_empty() {
            let mut app = TechnicalSessionApp {
                stage: TechnicalStage::Loading(LoadingApp {
                    message: "Generating technical backlog".to_string(),
                    detail: format!(
                        "Building `{}/backlog/_TEMPLATE` for {}.",
                        branding::PROJECT_DIR,
                        parent.identifier
                    ),
                    spinner_index: 0,
                }),
                copy: CopyUiState::default(),
                agent_overrides: agent_overrides.clone(),
                continuation: None,
                question_limit,
                refinement_round_limit,
                pending: None,
            };
            start_initial_generation(
                &mut app,
                root,
                TechnicalGenerationRequest {
                    parent,
                    selected_acceptance_criteria: Vec::new(),
                    discussion_budgets,
                },
                None,
            );
            app
        } else {
            TechnicalSessionApp {
                stage: TechnicalStage::SelectCriteria(AcceptanceCriteriaApp {
                    parent,
                    criteria: MultiSelectFieldState::new(criteria.clone(), 0..criteria.len()),
                    error: None,
                    sticky_error: false,
                }),
                copy: CopyUiState::default(),
                agent_overrides: agent_overrides.clone(),
                continuation: None,
                question_limit,
                refinement_round_limit,
                pending: None,
            }
        }
    } else {
        TechnicalSessionApp {
            stage: TechnicalStage::PickIssue(IssuePickerApp {
                query: InputFieldState::default(),
                issues,
                selected: 0,
                focus: IssuePickerFocus::List,
                preview_scroll: ScrollState::default(),
                error: None,
                sticky_error: false,
            }),
            copy: CopyUiState::default(),
            agent_overrides,
            continuation: None,
            question_limit,
            refinement_round_limit,
            pending: None,
        }
    };

    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture
    )?;
    let _cleanup = TerminalCleanup;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    loop {
        process_pending_generation(&mut app)?;
        advance_loading_spinner(&mut app);
        terminal.draw(|frame| render_technical_session(frame, &app))?;

        if event::poll(Duration::from_millis(if app.pending.is_some() {
            120
        } else {
            250
        }))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if app.pending.is_some() && top_level_cancel(key) {
                        return Ok(InteractiveTechnicalExit::Cancelled);
                    }

                    if key.code == KeyCode::Esc {
                        if let TechnicalStage::Refinement(refinement) = &app.stage {
                            app.stage = TechnicalStage::Review(refinement.review.clone());
                            continue;
                        }
                        return Ok(InteractiveTechnicalExit::Cancelled);
                    }

                    if app.pending.is_some() {
                        continue;
                    }

                    if is_mouse_toggle_key(key) {
                        app.copy.toggle_mouse_capture(terminal.backend_mut())?;
                        continue;
                    }

                    let terminal_area = terminal.size()?.into();
                    if app.copy.export_active()
                        && app
                            .copy
                            .handle_export_key(key, copy_overlay_viewport(terminal_area))
                    {
                        continue;
                    }

                    let action = match &mut app.stage {
                        TechnicalStage::PickIssue(picker) => handle_issue_picker_key(
                            picker,
                            &mut app.copy,
                            key,
                            issue_picker_preview_viewport(terminal_area),
                        ),
                        TechnicalStage::SelectCriteria(criteria) => handle_acceptance_criteria_key(
                            criteria,
                            key,
                            &mut app.copy,
                            discussion_budgets,
                        ),
                        TechnicalStage::Questions(questions) => handle_technical_questions_key(
                            questions,
                            &mut app.copy,
                            key,
                            technical_questions_answer_viewport(terminal_area),
                        ),
                        TechnicalStage::Refinement(refinement) => {
                            handle_technical_review_refinement_key(
                                refinement,
                                key,
                                technical_refinement_input_viewport(terminal_area),
                            )
                        }
                        TechnicalStage::Loading(_) => TechnicalAction::None,
                        TechnicalStage::Review(review) => handle_technical_review_key(
                            review,
                            &mut app.copy,
                            key,
                            technical_review_preview_viewport(terminal_area),
                            app.refinement_round_limit,
                        ),
                    };

                    match action {
                        TechnicalAction::None => {}
                        TechnicalAction::SelectIssue(parent) => {
                            let criteria =
                                extract_acceptance_criteria(parent.description.as_deref());
                            if criteria.is_empty() {
                                let previous_stage = app.stage.clone();
                                start_initial_generation(
                                    &mut app,
                                    root,
                                    TechnicalGenerationRequest {
                                        parent,
                                        selected_acceptance_criteria: Vec::new(),
                                        discussion_budgets,
                                    },
                                    Some(previous_stage),
                                );
                            } else {
                                app.stage = TechnicalStage::SelectCriteria(AcceptanceCriteriaApp {
                                    parent,
                                    criteria: MultiSelectFieldState::new(
                                        criteria.clone(),
                                        0..criteria.len(),
                                    ),
                                    error: None,
                                    sticky_error: false,
                                });
                            }
                        }
                        TechnicalAction::Generate(request) => {
                            let previous_stage = app.stage.clone();
                            start_initial_generation(&mut app, root, request, Some(previous_stage));
                        }
                        TechnicalAction::ContinueWithAnswers {
                            workflow,
                            follow_ups,
                        } => {
                            let previous_stage = app.stage.clone();
                            start_follow_up_generation(
                                &mut app,
                                root,
                                workflow,
                                follow_ups,
                                Some(previous_stage),
                            );
                        }
                        TechnicalAction::OpenRefinement { review } => {
                            app.stage =
                                TechnicalStage::Refinement(build_review_refinement_app(review));
                        }
                        TechnicalAction::Refine { review, addendum } => {
                            let previous_stage = app.stage.clone();
                            start_refinement_generation(
                                &mut app,
                                root,
                                review,
                                addendum,
                                Some(previous_stage),
                            );
                        }
                        TechnicalAction::Confirm(generated) => {
                            return Ok(InteractiveTechnicalExit::Confirmed(generated));
                        }
                    }
                }
                Event::Paste(text) => match &mut app.stage {
                    TechnicalStage::PickIssue(picker) => handle_issue_picker_paste(picker, &text),
                    TechnicalStage::Questions(questions) => {
                        handle_technical_questions_paste(questions, &text);
                    }
                    TechnicalStage::Refinement(refinement) => {
                        refinement.addendum.paste(&text);
                        refinement.error = None;
                    }
                    TechnicalStage::SelectCriteria(_)
                    | TechnicalStage::Loading(_)
                    | TechnicalStage::Review(_) => {}
                },
                Event::Mouse(mouse) => {
                    let terminal_area = terminal.size()?.into();
                    if app.copy.export_active() {
                        let _ = app
                            .copy
                            .handle_export_mouse(mouse, copy_overlay_viewport(terminal_area));
                        continue;
                    }
                    match &mut app.stage {
                        TechnicalStage::PickIssue(picker)
                            if picker.focus == IssuePickerFocus::Preview
                                && matches!(
                                    mouse.kind,
                                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                                ) =>
                        {
                            let viewport = issue_picker_preview_viewport(terminal_area);
                            let _ = picker.preview_scroll.apply_mouse_in_viewport(
                                mouse,
                                viewport,
                                picker.preview_content_rows(viewport.width.max(1)),
                            );
                        }
                        TechnicalStage::Questions(questions) => {
                            if let Some(question) = questions.questions.get_mut(questions.selected)
                            {
                                let viewport = technical_questions_answer_viewport(terminal_area);
                                let _ = handle_technical_questions_mouse(question, mouse, viewport);
                            }
                        }
                        TechnicalStage::Review(review)
                            if review.focus == TechnicalReviewFocus::Preview
                                && matches!(
                                    mouse.kind,
                                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                                ) =>
                        {
                            let viewport = technical_review_preview_viewport(terminal_area);
                            let _ = review.preview_scroll.apply_mouse_in_viewport(
                                mouse,
                                viewport,
                                review.preview_content_rows(viewport.width.max(1)),
                            );
                        }
                        TechnicalStage::Refinement(_) => {}
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
}

fn build_questions_app(
    workflow: TechnicalWorkflowState,
    questions: Vec<String>,
) -> TechnicalQuestionsApp {
    TechnicalQuestionsApp {
        workflow,
        questions: questions
            .into_iter()
            .map(|question| QuestionAnswer {
                question,
                answer: InputFieldState::multiline(String::new()),
                state: FollowUpAnswerState::Pending,
            })
            .collect(),
        selected: 0,
        error: None,
        sticky_error: false,
    }
}

fn build_review_app(workflow: TechnicalWorkflowState) -> Result<TechnicalReviewApp> {
    if workflow.files.is_empty() {
        bail!("technical backlog draft is empty");
    }
    Ok(TechnicalReviewApp {
        workflow,
        selected_file: 0,
        focus: TechnicalReviewFocus::Files,
        preview_scroll: ScrollState::default(),
        error: None,
    })
}

fn build_review_refinement_app(review: TechnicalReviewApp) -> TechnicalReviewRefinementApp {
    TechnicalReviewRefinementApp {
        review,
        addendum: InputFieldState::multiline(String::new()),
        error: None,
    }
}

fn set_stage_error(stage: &mut TechnicalStage, error: String) {
    match stage {
        TechnicalStage::PickIssue(picker) => {
            set_sticky_error(&mut picker.error, &mut picker.sticky_error, error);
        }
        TechnicalStage::SelectCriteria(criteria) => {
            set_sticky_error(&mut criteria.error, &mut criteria.sticky_error, error);
        }
        TechnicalStage::Questions(questions) => {
            set_sticky_error(&mut questions.error, &mut questions.sticky_error, error);
        }
        TechnicalStage::Review(review) => {
            review.error = Some(error);
        }
        TechnicalStage::Refinement(refinement) => {
            refinement.error = Some(error);
        }
        TechnicalStage::Loading(_) => {}
    }
}

fn start_initial_generation(
    app: &mut TechnicalSessionApp,
    root: &Path,
    request: TechnicalGenerationRequest,
    previous_stage: Option<TechnicalStage>,
) {
    app.stage = TechnicalStage::Loading(LoadingApp {
        message: "Generating technical backlog".to_string(),
        detail: format!(
            "Building `{}/backlog/_TEMPLATE` for {}.",
            branding::PROJECT_DIR,
            request.parent.identifier
        ),
        spinner_index: 0,
    });
    app.pending = Some(PendingTechnicalJob {
        receiver: spawn_initial_generation_job(
            root.to_path_buf(),
            request,
            app.question_limit,
            app.agent_overrides.clone(),
            app.continuation.clone(),
        ),
        previous_stage,
    });
}

fn start_follow_up_generation(
    app: &mut TechnicalSessionApp,
    root: &Path,
    workflow: TechnicalWorkflowState,
    follow_ups: Vec<TechnicalFollowUpResponse>,
    previous_stage: Option<TechnicalStage>,
) {
    app.stage = TechnicalStage::Loading(LoadingApp {
        message: format!(
            "Answering technical follow-up questions ({})",
            workflow.parent.identifier
        ),
        detail: format!(
            "Continuing the draft after {} follow-up answer(s).",
            follow_ups.len()
        ),
        spinner_index: 0,
    });
    app.pending = Some(PendingTechnicalJob {
        receiver: spawn_follow_up_generation_job(
            root.to_path_buf(),
            workflow,
            follow_ups,
            app.question_limit,
            app.agent_overrides.clone(),
            app.continuation.clone(),
        ),
        previous_stage,
    });
}

fn start_refinement_generation(
    app: &mut TechnicalSessionApp,
    root: &Path,
    review: TechnicalReviewApp,
    addendum: String,
    previous_stage: Option<TechnicalStage>,
) {
    app.stage = TechnicalStage::Loading(LoadingApp {
        message: format!(
            "Refining technical draft ({})",
            review.workflow.parent.identifier
        ),
        detail: "Rebuilding the draft with your refinement guidance.".to_string(),
        spinner_index: 0,
    });
    app.pending = Some(PendingTechnicalJob {
        receiver: spawn_refinement_job(
            root.to_path_buf(),
            review,
            addendum,
            app.question_limit,
            app.agent_overrides.clone(),
            app.continuation.clone(),
        ),
        previous_stage,
    });
}

fn spawn_initial_generation_job(
    root: PathBuf,
    request: TechnicalGenerationRequest,
    question_limit: usize,
    agent_overrides: TechnicalAgentOverrides,
    continuation: Option<AgentContinuation>,
) -> Receiver<TechnicalWorkerReport> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut continuation = continuation;
        let outcome = build_technical_workflow_state(
            &root,
            &request.parent,
            &request.selected_acceptance_criteria,
            request.discussion_budgets,
        )
        .and_then(|workflow| {
            generate_technical_route_outcome(
                &root,
                workflow,
                TechnicalPromptKind::Initial,
                question_limit,
                &agent_overrides,
                &mut continuation,
            )
        })
        .and_then(|outcome| match outcome {
            TechnicalRouteOutcome::Questions {
                workflow,
                questions,
            } => Ok(TechnicalWorkerOutcome::Questions(build_questions_app(
                workflow, questions,
            ))),
            TechnicalRouteOutcome::Draft(workflow) => {
                Ok(TechnicalWorkerOutcome::Review(build_review_app(workflow)?))
            }
        });
        let _ = sender.send(TechnicalWorkerReport {
            continuation,
            outcome,
        });
    });
    receiver
}

fn spawn_follow_up_generation_job(
    root: PathBuf,
    mut workflow: TechnicalWorkflowState,
    follow_ups: Vec<TechnicalFollowUpResponse>,
    question_limit: usize,
    agent_overrides: TechnicalAgentOverrides,
    continuation: Option<AgentContinuation>,
) -> Receiver<TechnicalWorkerReport> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut continuation = continuation;
        workflow.follow_ups.extend(follow_ups);
        let remaining_questions = question_limit.saturating_sub(workflow.questions_asked);
        let outcome = generate_technical_route_outcome(
            &root,
            workflow,
            TechnicalPromptKind::FollowUp,
            remaining_questions,
            &agent_overrides,
            &mut continuation,
        )
        .and_then(|outcome| match outcome {
            TechnicalRouteOutcome::Questions {
                workflow,
                questions,
            } => Ok(TechnicalWorkerOutcome::Questions(build_questions_app(
                workflow, questions,
            ))),
            TechnicalRouteOutcome::Draft(workflow) => {
                Ok(TechnicalWorkerOutcome::Review(build_review_app(workflow)?))
            }
        });
        let _ = sender.send(TechnicalWorkerReport {
            continuation,
            outcome,
        });
    });
    receiver
}

fn spawn_refinement_job(
    root: PathBuf,
    review: TechnicalReviewApp,
    addendum: String,
    question_limit: usize,
    agent_overrides: TechnicalAgentOverrides,
    continuation: Option<AgentContinuation>,
) -> Receiver<TechnicalWorkerReport> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut continuation = continuation;
        let mut workflow = review.workflow;
        workflow.refinement_history.push(addendum.clone());
        let remaining_questions = question_limit.saturating_sub(workflow.questions_asked);
        let outcome = generate_technical_route_outcome(
            &root,
            workflow,
            TechnicalPromptKind::Refinement { addendum },
            remaining_questions,
            &agent_overrides,
            &mut continuation,
        )
        .and_then(|outcome| match outcome {
            TechnicalRouteOutcome::Questions {
                workflow,
                questions,
            } => Ok(TechnicalWorkerOutcome::Questions(build_questions_app(
                workflow, questions,
            ))),
            TechnicalRouteOutcome::Draft(workflow) => {
                Ok(TechnicalWorkerOutcome::Review(build_review_app(workflow)?))
            }
        });
        let _ = sender.send(TechnicalWorkerReport {
            continuation,
            outcome,
        });
    });
    receiver
}

fn handle_issue_picker_key(
    app: &mut IssuePickerApp,
    copy: &mut CopyUiState,
    key: crossterm::event::KeyEvent,
    preview_viewport: Rect,
) -> TechnicalAction {
    match key.code {
        KeyCode::Tab => {
            app.focus = match app.focus {
                IssuePickerFocus::List => IssuePickerFocus::Preview,
                IssuePickerFocus::Preview => IssuePickerFocus::List,
            };
            clear_error_for_navigation(&mut app.error, &app.sticky_error);
            TechnicalAction::None
        }
        KeyCode::Up => {
            if app.focus == IssuePickerFocus::Preview {
                let _ = app.preview_scroll.apply_key_code_in_viewport(
                    KeyCode::Up,
                    preview_viewport,
                    app.preview_content_rows(preview_viewport.width.max(1)),
                );
            } else {
                let filtered = search_results(app);
                if filtered.is_empty() {
                    app.selected = 0;
                } else if app.selected == 0 {
                    app.selected = filtered.len().saturating_sub(1);
                } else {
                    app.selected -= 1;
                }
                app.preview_scroll.reset();
            }
            clear_error_for_navigation(&mut app.error, &app.sticky_error);
            TechnicalAction::None
        }
        KeyCode::Down => {
            if app.focus == IssuePickerFocus::Preview {
                let _ = app.preview_scroll.apply_key_code_in_viewport(
                    KeyCode::Down,
                    preview_viewport,
                    app.preview_content_rows(preview_viewport.width.max(1)),
                );
            } else {
                let filtered = search_results(app);
                if filtered.is_empty() {
                    app.selected = 0;
                } else {
                    app.selected = (app.selected + 1) % filtered.len();
                }
                app.preview_scroll.reset();
            }
            clear_error_for_navigation(&mut app.error, &app.sticky_error);
            TechnicalAction::None
        }
        KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End
            if app.focus == IssuePickerFocus::Preview =>
        {
            let _ = app.preview_scroll.apply_key_in_viewport(
                key,
                preview_viewport,
                app.preview_content_rows(preview_viewport.width.max(1)),
            );
            clear_error_for_navigation(&mut app.error, &app.sticky_error);
            TechnicalAction::None
        }
        KeyCode::Enter => {
            let filtered = search_results(app);
            let Some(issue_index) = filtered.get(app.selected).map(|result| result.issue_index)
            else {
                set_transient_error(
                    &mut app.error,
                    &mut app.sticky_error,
                    "No issues match the current search.".to_string(),
                );
                return TechnicalAction::None;
            };
            clear_error(&mut app.error, &mut app.sticky_error);
            TechnicalAction::SelectIssue(app.issues[issue_index].clone())
        }
        _ => {
            if is_copy_key(key) {
                match app.focus {
                    IssuePickerFocus::List => {
                        copy.copy_payload(app.query.copy_payload("technical parent issue search"));
                    }
                    IssuePickerFocus::Preview => {
                        copy.copy_payload(issue_picker_preview_payload(app));
                    }
                }
            } else if app.focus == IssuePickerFocus::List && app.query.handle_key(key) {
                app.selected = 0;
                app.preview_scroll.reset();
                if input_key_clears_sticky_error(key) {
                    clear_error(&mut app.error, &mut app.sticky_error);
                } else {
                    clear_error_for_navigation(&mut app.error, &app.sticky_error);
                }
            }
            TechnicalAction::None
        }
    }
}

fn handle_issue_picker_paste(app: &mut IssuePickerApp, text: &str) {
    if app.focus == IssuePickerFocus::List && app.query.paste(text) {
        app.selected = 0;
        app.preview_scroll.reset();
        clear_error(&mut app.error, &mut app.sticky_error);
    }
}

fn handle_acceptance_criteria_key(
    app: &mut AcceptanceCriteriaApp,
    key: crossterm::event::KeyEvent,
    copy: &mut CopyUiState,
    discussion_budgets: TicketDiscussionBudgets,
) -> TechnicalAction {
    match key.code {
        KeyCode::Enter => {
            let selected_acceptance_criteria = app
                .criteria
                .selected_labels()
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>();
            if selected_acceptance_criteria.is_empty() {
                set_transient_error(
                    &mut app.error,
                    &mut app.sticky_error,
                    "Select at least one acceptance criterion before generating the technical backlog."
                        .to_string(),
                );
                return TechnicalAction::None;
            }
            clear_error(&mut app.error, &mut app.sticky_error);
            TechnicalAction::Generate(TechnicalGenerationRequest {
                parent: app.parent.clone(),
                selected_acceptance_criteria,
                discussion_budgets,
            })
        }
        _ if is_copy_key(key) => {
            copy.copy_payload(acceptance_criteria_copy_payload(app));
            TechnicalAction::None
        }
        _ => {
            if app.criteria.handle_key(key) {
                if matches!(key.code, KeyCode::Char(' ')) {
                    clear_error(&mut app.error, &mut app.sticky_error);
                } else {
                    clear_error_for_navigation(&mut app.error, &app.sticky_error);
                }
            }
            TechnicalAction::None
        }
    }
}

fn handle_technical_review_key(
    app: &mut TechnicalReviewApp,
    copy: &mut CopyUiState,
    key: crossterm::event::KeyEvent,
    preview_viewport: Rect,
    refinement_round_limit: usize,
) -> TechnicalAction {
    match key.code {
        KeyCode::BackTab => {
            app.focus = match app.focus {
                TechnicalReviewFocus::Files => TechnicalReviewFocus::Preview,
                TechnicalReviewFocus::Preview => TechnicalReviewFocus::Files,
            };
            app.error = None;
            TechnicalAction::None
        }
        KeyCode::Tab => {
            app.focus = match app.focus {
                TechnicalReviewFocus::Files => TechnicalReviewFocus::Preview,
                TechnicalReviewFocus::Preview => TechnicalReviewFocus::Files,
            };
            app.error = None;
            TechnicalAction::None
        }
        KeyCode::Up => {
            if app.focus == TechnicalReviewFocus::Preview {
                let _ = app.preview_scroll.apply_key_code_in_viewport(
                    KeyCode::Up,
                    preview_viewport,
                    app.preview_content_rows(preview_viewport.width.max(1)),
                );
            } else if app.selected_file == 0 {
                app.selected_file = app.workflow.files.len().saturating_sub(1);
            } else {
                app.selected_file -= 1;
                app.preview_scroll.reset();
            }
            app.error = None;
            TechnicalAction::None
        }
        KeyCode::Down => {
            if app.focus == TechnicalReviewFocus::Preview {
                let _ = app.preview_scroll.apply_key_code_in_viewport(
                    KeyCode::Down,
                    preview_viewport,
                    app.preview_content_rows(preview_viewport.width.max(1)),
                );
            } else if !app.workflow.files.is_empty() {
                app.selected_file = (app.selected_file + 1) % app.workflow.files.len();
                app.preview_scroll.reset();
            }
            app.error = None;
            TechnicalAction::None
        }
        KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End
            if app.focus == TechnicalReviewFocus::Preview =>
        {
            let _ = app.preview_scroll.apply_key_in_viewport(
                key,
                preview_viewport,
                app.preview_content_rows(preview_viewport.width.max(1)),
            );
            app.error = None;
            TechnicalAction::None
        }
        KeyCode::Char('f') => {
            if app.workflow.refinement_history.len() >= refinement_round_limit {
                app.error = Some(format!(
                    "technical refinement limit reached ({refinement_round_limit}); confirm the current draft or increase the configured limit"
                ));
                TechnicalAction::None
            } else {
                app.error = None;
                TechnicalAction::OpenRefinement {
                    review: app.clone(),
                }
            }
        }
        KeyCode::Enter => match app.workflow.to_generated_backlog() {
            Ok(generated) => TechnicalAction::Confirm(generated),
            Err(error) => {
                app.error = Some(error.to_string());
                TechnicalAction::None
            }
        },
        _ if is_copy_key(key) => {
            copy.copy_payload(technical_review_copy_payload(app));
            TechnicalAction::None
        }
        _ => TechnicalAction::None,
    }
}

fn handle_technical_questions_key(
    app: &mut TechnicalQuestionsApp,
    copy: &mut CopyUiState,
    key: crossterm::event::KeyEvent,
    input_viewport: Rect,
) -> TechnicalAction {
    match key.code {
        KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(question) = app.questions.get_mut(app.selected) {
                match question.answer.paste_clipboard_with_prompt_attachments() {
                    Ok(_) => {
                        question.state = FollowUpAnswerState::Pending;
                        clear_error(&mut app.error, &mut app.sticky_error);
                    }
                    Err(error) => set_transient_error(
                        &mut app.error,
                        &mut app.sticky_error,
                        error.to_string(),
                    ),
                }
            }
            TechnicalAction::None
        }
        KeyCode::BackTab => {
            if app.selected == 0 {
                app.selected = app.questions.len().saturating_sub(1);
            } else {
                app.selected -= 1;
            }
            clear_error_for_navigation(&mut app.error, &app.sticky_error);
            TechnicalAction::None
        }
        KeyCode::Tab => {
            app.selected = (app.selected + 1) % app.questions.len();
            clear_error_for_navigation(&mut app.error, &app.sticky_error);
            TechnicalAction::None
        }
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let Some(selected) = app.questions.get_mut(app.selected) else {
                return TechnicalAction::None;
            };
            selected.state = if selected.answer.display_value().trim().is_empty() {
                FollowUpAnswerState::Skipped
            } else {
                FollowUpAnswerState::Answered
            };
            if app.questions.iter().all(question_is_completed) {
                clear_error(&mut app.error, &mut app.sticky_error);
                TechnicalAction::ContinueWithAnswers {
                    workflow: app.workflow.clone(),
                    follow_ups: collect_follow_up_responses(&app.questions),
                }
            } else {
                if let Some(index) = next_incomplete_question(&app.questions, app.selected) {
                    app.selected = index;
                }
                clear_error_for_navigation(&mut app.error, &app.sticky_error);
                TechnicalAction::None
            }
        }
        KeyCode::Enter => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                if let Some(question) = app.questions.get_mut(app.selected)
                    && question.answer.insert_newline()
                {
                    question.state = FollowUpAnswerState::Pending;
                    clear_error(&mut app.error, &mut app.sticky_error);
                }
                return TechnicalAction::None;
            }

            let Some(selected) = app.questions.get_mut(app.selected) else {
                return TechnicalAction::None;
            };
            selected.state = if selected.answer.display_value().trim().is_empty() {
                FollowUpAnswerState::Skipped
            } else {
                FollowUpAnswerState::Answered
            };
            if app.questions.iter().all(question_is_completed) {
                clear_error(&mut app.error, &mut app.sticky_error);
                TechnicalAction::ContinueWithAnswers {
                    workflow: app.workflow.clone(),
                    follow_ups: collect_follow_up_responses(&app.questions),
                }
            } else {
                if let Some(index) = next_incomplete_question(&app.questions, app.selected) {
                    app.selected = index;
                }
                clear_error_for_navigation(&mut app.error, &app.sticky_error);
                TechnicalAction::None
            }
        }
        _ => {
            if is_copy_key(key) {
                copy.copy_payload(technical_questions_copy_payload(app));
            } else if let Some(question) = app.questions.get_mut(app.selected)
                && question.answer.handle_key_with_viewport(
                    key,
                    input_viewport.width,
                    input_viewport.height,
                )
            {
                question.state = FollowUpAnswerState::Pending;
                if input_key_clears_sticky_error(key) {
                    clear_error(&mut app.error, &mut app.sticky_error);
                } else {
                    clear_error_for_navigation(&mut app.error, &app.sticky_error);
                }
            }
            TechnicalAction::None
        }
    }
}

fn handle_technical_questions_paste(app: &mut TechnicalQuestionsApp, text: &str) {
    if let Some(question) = app.questions.get_mut(app.selected) {
        match question.answer.paste_with_prompt_attachments(text) {
            Ok(_) => {
                question.state = FollowUpAnswerState::Pending;
                clear_error(&mut app.error, &mut app.sticky_error);
            }
            Err(error) => {
                set_transient_error(&mut app.error, &mut app.sticky_error, error.to_string());
            }
        }
    }
}

fn handle_technical_questions_mouse(
    question: &mut QuestionAnswer,
    mouse: MouseEvent,
    input_viewport: Rect,
) -> bool {
    if !matches!(
        mouse.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) {
        return false;
    }

    question.answer.handle_mouse_scroll(
        mouse,
        input_viewport,
        input_viewport.width,
        input_viewport.height,
    )
}

fn handle_technical_review_refinement_key(
    app: &mut TechnicalReviewRefinementApp,
    key: crossterm::event::KeyEvent,
    input_viewport: Rect,
) -> TechnicalAction {
    match key.code {
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let addendum = app.addendum.display_value().trim().to_string();
            if addendum.is_empty() {
                app.error = Some("Enter the refinement guidance before continuing.".to_string());
                TechnicalAction::None
            } else {
                app.error = None;
                TechnicalAction::Refine {
                    review: app.review.clone(),
                    addendum,
                }
            }
        }
        KeyCode::Enter => {
            if app.addendum.insert_newline() {
                app.error = None;
            }
            TechnicalAction::None
        }
        _ => {
            if app.addendum.handle_key_with_viewport(
                key,
                input_viewport.width,
                input_viewport.height,
            ) {
                app.error = None;
            }
            TechnicalAction::None
        }
    }
}

fn question_is_completed(question: &QuestionAnswer) -> bool {
    matches!(
        question.state,
        FollowUpAnswerState::Answered | FollowUpAnswerState::Skipped
    )
}

fn next_incomplete_question(questions: &[QuestionAnswer], selected: usize) -> Option<usize> {
    if questions.is_empty() {
        return None;
    }
    let len = questions.len();
    (1..=len)
        .map(|offset| (selected + offset) % len)
        .find(|index| !question_is_completed(&questions[*index]))
}

fn collect_follow_up_responses(questions: &[QuestionAnswer]) -> Vec<TechnicalFollowUpResponse> {
    questions
        .iter()
        .map(|question| TechnicalFollowUpResponse {
            question: question.question.clone(),
            answer: question.answer.display_value().trim().to_string(),
            skipped: question.state == FollowUpAnswerState::Skipped,
        })
        .collect()
}

impl IssuePickerApp {
    fn preview_content_rows(&self, width: u16) -> usize {
        let preview = search_results(self)
            .get(self.selected)
            .and_then(|result| {
                self.issues.get(result.issue_index).map(|issue| {
                    render_issue_preview(
                        issue,
                        Some(result),
                        None,
                        "_No Linear description was provided._",
                    )
                })
            })
            .unwrap_or_else(|| {
                Text::from(vec![
                    Line::from("Search results appear here."),
                    Line::from(""),
                    Line::from(
                        "Type to narrow the ticket list, then press Enter to generate the technical backlog draft.",
                    ),
                ])
            });
        wrapped_rows(&plain_text(&preview), width.max(1))
    }
}

impl TechnicalQuestionsApp {
    fn answered_count(&self) -> usize {
        self.questions
            .iter()
            .filter(|question| question.state == FollowUpAnswerState::Answered)
            .count()
    }

    fn skipped_count(&self) -> usize {
        self.questions
            .iter()
            .filter(|question| question.state == FollowUpAnswerState::Skipped)
            .count()
    }
}

impl TechnicalReviewApp {
    fn preview_text(&self) -> Text<'static> {
        self.workflow
            .files
            .get(self.selected_file)
            .map(|file| render_markdown(&file.contents, Style::default(), &[]))
            .unwrap_or_else(|| Text::from(""))
    }

    fn preview_content_rows(&self, width: u16) -> usize {
        wrapped_rows(&plain_text(&self.preview_text()), width.max(1))
    }
}

fn issue_picker_preview_payload(app: &IssuePickerApp) -> CopyPayload {
    let preview = search_results(app).get(app.selected).and_then(|result| {
        app.issues.get(result.issue_index).map(|issue| {
            let plain_text = plain_text(&render_issue_preview(
                issue,
                Some(result),
                None,
                "_No Linear description was provided._",
            ));
            CopyPayload::with_markdown(
                "technical parent issue preview",
                plain_text,
                issue.description.clone().unwrap_or_default(),
            )
        })
    });

    preview.unwrap_or_else(|| {
        CopyPayload::new(
            "technical parent issue preview",
            "Search results appear here.\n\nType to narrow the ticket list, then press Enter to generate the technical backlog draft.",
        )
    })
}

fn acceptance_criteria_copy_payload(app: &AcceptanceCriteriaApp) -> CopyPayload {
    let mut lines = vec![
        format!("Parent: {}", app.parent.identifier),
        app.parent.title.clone(),
        String::new(),
        "Selected criteria".to_string(),
    ];
    if app.criteria.selected_labels().is_empty() {
        lines.push("_No acceptance criteria selected yet._".to_string());
    } else {
        lines.extend(
            app.criteria
                .selected_labels()
                .into_iter()
                .map(|criterion| format!("- {criterion}")),
        );
    }
    CopyPayload::new("technical criteria summary", lines.join("\n"))
}

fn technical_questions_copy_payload(app: &TechnicalQuestionsApp) -> CopyPayload {
    let mut lines = vec![
        format!("Parent: {}", app.workflow.parent.identifier),
        app.workflow.parent.title.clone(),
        String::new(),
    ];
    if let Some(selected) = app.questions.get(app.selected) {
        lines.push(format!(
            "Question {}\n\n{}",
            app.selected + 1,
            selected.question
        ));
        lines.push(String::new());
        lines.push(format!(
            "Answer {}\n\n{}",
            app.selected + 1,
            selected.answer.copy_payload("technical answer").plain_text
        ));
    }
    CopyPayload::new("technical follow-up", lines.join("\n"))
}

fn technical_review_file_list_text(app: &TechnicalReviewApp) -> Text<'static> {
    Text::from(
        app.workflow
            .files
            .iter()
            .map(|file| Line::from(file.relative_path.clone()))
            .collect::<Vec<_>>(),
    )
}

fn technical_review_copy_payload(app: &TechnicalReviewApp) -> CopyPayload {
    match app.focus {
        TechnicalReviewFocus::Files => CopyPayload::from_text(
            "technical generated file list",
            technical_review_file_list_text(app),
        ),
        TechnicalReviewFocus::Preview => {
            let selected = &app.workflow.files[app.selected_file];
            CopyPayload::markdown(
                format!("technical preview {}", selected.relative_path),
                selected.contents.clone(),
            )
        }
    }
}

fn process_pending_generation(app: &mut TechnicalSessionApp) -> Result<()> {
    let Some(pending) = app.pending.as_ref() else {
        return Ok(());
    };

    match pending.receiver.try_recv() {
        Ok(result) => {
            let pending = app
                .pending
                .take()
                .ok_or_else(|| anyhow!("technical generation job disappeared unexpectedly"))?;
            app.continuation = result.continuation;
            match result.outcome {
                Ok(TechnicalWorkerOutcome::Questions(questions)) => {
                    app.stage = TechnicalStage::Questions(questions);
                }
                Ok(TechnicalWorkerOutcome::Review(review)) => {
                    app.stage = TechnicalStage::Review(review);
                }
                Err(error) => {
                    if let Some(mut previous_stage) = pending.previous_stage {
                        set_stage_error(&mut previous_stage, error.to_string());
                        app.stage = previous_stage;
                    } else {
                        return Err(error);
                    }
                }
            }
        }
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => {
            let pending = app
                .pending
                .take()
                .ok_or_else(|| anyhow!("technical generation job disappeared unexpectedly"))?;
            if let Some(mut previous_stage) = pending.previous_stage {
                set_stage_error(
                    &mut previous_stage,
                    "technical generation worker exited before returning a result".to_string(),
                );
                app.stage = previous_stage;
            } else {
                bail!("technical generation worker exited before returning a result");
            }
        }
    }

    Ok(())
}

fn advance_loading_spinner(app: &mut TechnicalSessionApp) {
    if let TechnicalStage::Loading(loading) = &mut app.stage {
        loading.spinner_index = (loading.spinner_index + 1) % SPINNER_FRAMES.len();
    }
}

fn build_technical_workflow_state(
    root: &Path,
    parent: &IssueSummary,
    selected_acceptance_criteria: &[String],
    discussion_budgets: TicketDiscussionBudgets,
) -> Result<TechnicalWorkflowState> {
    let prepared_context = prepare_issue_context(parent, discussion_budgets);
    let child_title = format!("Technical: {}", parent.title);
    let template_files = render_template_files(
        root,
        &TemplateContext {
            issue_title: Some(child_title.clone()),
            parent_identifier: Some(parent.identifier.clone()),
            parent_title: Some(parent.title.clone()),
            parent_url: Some(parent.url.clone()),
            parent_description: prepared_context.issue.description.clone(),
            ..TemplateContext::default()
        },
    )?;
    Ok(TechnicalWorkflowState {
        parent: parent.clone(),
        child_title,
        selected_acceptance_criteria: selected_acceptance_criteria.to_vec(),
        prepared_context,
        template_files,
        backlog_slug: slugify(&format!("Technical: {}", parent.title)),
        today: current_local_date()?,
        follow_ups: Vec::new(),
        questions_asked: 0,
        refinement_history: Vec::new(),
        files: Vec::new(),
        revision: 0,
    })
}

fn rendered_index_contents(rendered_files: &[RenderedTemplateFile]) -> Result<String> {
    rendered_files
        .iter()
        .find(|file| file.relative_path == INDEX_FILE_NAME)
        .map(|file| file.contents.clone())
        .ok_or_else(|| anyhow!("the technical backlog template must contain `{INDEX_FILE_NAME}`"))
}

#[derive(Debug, Clone)]
enum TechnicalPromptKind {
    Initial,
    FollowUp,
    Refinement { addendum: String },
}

impl TechnicalPromptKind {
    fn phase_name(&self) -> &'static str {
        match self {
            Self::Initial => "technical backlog generation",
            Self::FollowUp => "technical backlog follow-up",
            Self::Refinement { .. } => "technical backlog refinement",
        }
    }
}

enum TechnicalRouteOutcome {
    Questions {
        workflow: TechnicalWorkflowState,
        questions: Vec<String>,
    },
    Draft(TechnicalWorkflowState),
}

fn generate_technical_route_outcome(
    root: &Path,
    workflow: TechnicalWorkflowState,
    prompt_kind: TechnicalPromptKind,
    remaining_questions: usize,
    overrides: &TechnicalAgentOverrides,
    continuation: &mut Option<AgentContinuation>,
) -> Result<TechnicalRouteOutcome> {
    let prompt = render_technical_prompt(root, &workflow, &prompt_kind, remaining_questions)?;
    let output = run_agent_capture_with_continuation(
        &RunAgentArgs {
            root: Some(root.to_path_buf()),
            route_key: Some(AGENT_ROUTE_BACKLOG_TECH.to_string()),
            agent: overrides.agent.clone(),
            prompt,
            instructions: None,
            model: overrides.model.clone(),
            reasoning: overrides.reasoning.clone(),
            transport: None,
            attachments: Vec::new(),
        },
        continuation,
    )
    .with_context(|| {
        format!(
            "{} backlog tech requires a configured local agent to generate backlog content from `{}/backlog/_TEMPLATE`",
            branding::COMMAND_NAME,
            branding::PROJECT_DIR
        )
    })?;
    match parse_technical_route_response(&output.stdout, prompt_kind.phase_name())? {
        ParsedTechnicalRouteResponse::Questions { questions } => {
            let questions = questions
                .into_iter()
                .map(|question| question.trim().to_string())
                .filter(|question| !question.is_empty())
                .collect::<Vec<_>>();
            if questions.is_empty() {
                bail!(
                    "technical backlog agent returned `kind = questions` without any questions during {}",
                    prompt_kind.phase_name()
                );
            }
            if questions.len() > remaining_questions {
                bail!(
                    "technical backlog agent requested {} follow-up question(s), exceeding the remaining technical follow-up limit of {}",
                    questions.len(),
                    remaining_questions
                );
            }
            let mut workflow = workflow;
            workflow.questions_asked += questions.len();
            Ok(TechnicalRouteOutcome::Questions {
                workflow,
                questions,
            })
        }
        ParsedTechnicalRouteResponse::Draft { files } => {
            let files = validate_generated_files(files, &workflow.template_files)?;
            let mut workflow = workflow;
            workflow.files = files;
            workflow.revision += 1;
            Ok(TechnicalRouteOutcome::Draft(workflow))
        }
    }
}

#[derive(Debug)]
enum ParsedTechnicalRouteResponse {
    Questions { questions: Vec<String> },
    Draft { files: Vec<TechnicalBacklogFile> },
}

fn parse_technical_route_response(raw: &str, phase: &str) -> Result<ParsedTechnicalRouteResponse> {
    let value: Value = parse_agent_json(raw, phase)?;
    let kind = value.get("kind").and_then(Value::as_str).ok_or_else(|| {
        anyhow!("technical backlog agent response missing string `kind` during {phase}")
    })?;

    match kind {
        "questions" => {
            let questions = value
                .get("questions")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("technical backlog agent response missing `questions` array during {phase}"))?
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    item.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| anyhow!(
                            "technical backlog agent response contained a non-string entry in `questions[{index}]` during {phase}"
                        ))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(ParsedTechnicalRouteResponse::Questions { questions })
        }
        "draft" => {
            let files = serde_json::from_value::<Vec<TechnicalBacklogFile>>(
                value.get("files").cloned().ok_or_else(|| {
                    anyhow!("technical backlog agent response missing `files` array during {phase}")
                })?,
            )
            .with_context(|| {
                format!("technical backlog agent response contained invalid `files` during {phase}")
            })?;
            Ok(ParsedTechnicalRouteResponse::Draft { files })
        }
        other => bail!(
            "technical backlog agent response returned unsupported `kind` `{other}` during {phase}"
        ),
    }
}

fn render_follow_up_block(follow_ups: &[TechnicalFollowUpResponse]) -> String {
    if follow_ups.is_empty() {
        "No follow-up answers have been provided yet.".to_string()
    } else {
        follow_ups
            .iter()
            .enumerate()
            .map(|(index, follow_up)| {
                let answer = if follow_up.skipped {
                    SKIPPED_TECHNICAL_FOLLOW_UP_LABEL.to_string()
                } else {
                    follow_up.answer.clone()
                };
                format!("{}. Q: {}\n   A: {}", index + 1, follow_up.question, answer)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn render_refinement_history_block(history: &[String]) -> String {
    if history.is_empty() {
        "No refinement guidance has been provided yet.".to_string()
    } else {
        history
            .iter()
            .enumerate()
            .map(|(index, addendum)| format!("{}. {}", index + 1, addendum))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

fn render_file_block(files: &[RenderedTemplateFile], empty_message: &str) -> String {
    if files.is_empty() {
        return empty_message.to_string();
    }

    files
        .iter()
        .map(|file| {
            format!(
                "### `{}`\n```md\n{}\n```",
                file.relative_path, file.contents
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_technical_prompt(
    root: &Path,
    workflow: &TechnicalWorkflowState,
    prompt_kind: &TechnicalPromptKind,
    remaining_questions: usize,
) -> Result<String> {
    let context = load_context_bundle(root)?;
    let workflow_contract = load_workflow_contract(root)?;
    let repository_snapshot = render_repository_snapshot(root)?;
    let acceptance_criteria_block = if workflow.selected_acceptance_criteria.is_empty() {
        "_No acceptance criteria were selected for this technical sub-ticket._".to_string()
    } else {
        workflow
            .selected_acceptance_criteria
            .iter()
            .map(|criterion| format!("- {criterion}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let template_block = render_file_block(
        &workflow.template_files,
        "_No backlog template files were found._",
    );
    let parent = &workflow.prepared_context.issue;
    let parent_description_block = parent
        .description
        .as_deref()
        .unwrap_or("_No Linear description was provided._");
    let parent_context_block = parent
        .parent
        .as_ref()
        .and_then(|issue| issue.description.as_deref())
        .unwrap_or("_No parent description was provided._");
    let discussion_block = if workflow
        .prepared_context
        .prompt_discussion
        .trim()
        .is_empty()
    {
        "_No Linear comments were provided._".to_string()
    } else {
        workflow.prepared_context.prompt_discussion.clone()
    };
    let image_summary = render_ticket_image_summary(&workflow.prepared_context.images);
    let follow_up_block = render_follow_up_block(&workflow.follow_ups);
    let refinement_history_block = render_refinement_history_block(&workflow.refinement_history);
    let current_draft_block =
        render_file_block(&workflow.files, "_No current technical draft exists yet._");
    let question_guidance = if remaining_questions == 0 {
        "Do not ask follow-up questions. Return a complete technical draft.".to_string()
    } else {
        format!(
            "If more information is still required, ask at most {remaining_questions} concise follow-up question(s). Otherwise return a complete technical draft."
        )
    };
    let phase_intro = match prompt_kind {
        TechnicalPromptKind::Initial => {
            "You are starting a technical backlog draft for the active repository.".to_string()
        }
        TechnicalPromptKind::FollowUp => {
            "Continue the same technical backlog draft for the active repository.".to_string()
        }
        TechnicalPromptKind::Refinement { addendum } => format!(
            "Continue the same technical backlog review for the active repository.\n\nNew refinement guidance:\n{}",
            addendum
        ),
    };

    Ok(format!(
        "{phase_intro}\n\n\
Injected workflow contract:\n{workflow_contract}\n\n\
Parent Linear issue:\n\
- Identifier: `{}`\n\
- Title: {}\n\
- State: {}\n\
- URL: {}\n\
- Description:\n{}\n\n\
Parent issue context:\n{}\n\n\
Ticket discussion context:\n{}\n\n\
Localized ticket images:\n{}\n\n\
Derived backlog values:\n\
- `BACKLOG_TITLE`: {}\n\
- `BACKLOG_SLUG`: {}\n\
- `TODAY`: {}\n\n\
Selected acceptance criteria for this technical sub-ticket:\n\
{}\n\n\
Follow-up answers so far:\n{}\n\n\
Refinement guidance history:\n{}\n\n\
Current draft files:\n{}\n\n\
Repository planning context:\n{}\n\n\
Repository directory snapshot:\n{}\n\n\
Template files to convert into a concrete backlog item:\n{}\n\n\
Instructions:\n\
1. Produce concrete Markdown content for every listed template file.\n\
2. Preserve the file paths exactly as provided.\n\
3. Use the template structure as guidance, but replace placeholder prose with issue-specific, repo-specific content for the target repository only.\n\
4. Do not leave unresolved placeholders such as `{{BACKLOG_TITLE}}`, `{{BACKLOG_SLUG}}`, `{{TODAY}}`, `{{issue_title}}`, or `{{parent_identifier}}`.\n\
5. Keep links relative to the file that contains them.\n\
6. Default scope to the full repository root unless the user explicitly requested a narrower subproject, and create backlog content only for work inside this repository directory.\n\
7. {question_guidance}\n\
8. Return JSON only using exactly one of these tagged response shapes:\n\
{{\"kind\":\"questions\",\"questions\":[\"Question 1\",\"Question 2\"]}}\n\
{{\"kind\":\"draft\",\"files\":[{{\"path\":\"index.md\",\"contents\":\"# ...\"}}]}}\n\
9. When `kind` is `questions`, include at least one non-empty question and do not include `files`.\n\
10. When `kind` is `draft`, include every template file exactly once and do not include `questions`.",
        parent.identifier,
        parent.title,
        parent
            .state
            .as_ref()
            .map(|state| state.name.as_str())
            .unwrap_or("Unknown"),
        parent.url,
        parent_description_block,
        parent_context_block,
        discussion_block,
        image_summary,
        workflow.child_title,
        workflow.backlog_slug,
        workflow.today,
        acceptance_criteria_block,
        follow_up_block,
        refinement_history_block,
        current_draft_block,
        context,
        repository_snapshot,
        template_block,
    ))
}

fn resolve_ticket_discussion_budgets(
    planning_meta: &crate::config::PlanningMeta,
) -> TicketDiscussionBudgets {
    TicketDiscussionBudgets {
        prompt_chars: planning_meta
            .linear
            .ticket_context
            .discussion_prompt_chars
            .unwrap_or(TicketDiscussionBudgets::default().prompt_chars),
        persisted_chars: planning_meta
            .linear
            .ticket_context
            .discussion_persisted_chars
            .unwrap_or(TicketDiscussionBudgets::default().persisted_chars),
    }
}

fn log_ticket_image_download_failures(
    identifier: &str,
    failures: &[crate::linear::TicketImageDownloadFailure],
) {
    for failure in failures {
        eprintln!(
            "warning: failed to localize ticket image for {identifier}: {} from {} ({})",
            failure.filename, failure.source_label, failure.error
        );
    }
}

fn validate_generated_files(
    generated_files: Vec<TechnicalBacklogFile>,
    template_files: &[RenderedTemplateFile],
) -> Result<Vec<RenderedTemplateFile>> {
    let expected_paths = template_files
        .iter()
        .map(|file| file.relative_path.clone())
        .collect::<BTreeSet<_>>();
    let mut actual_files = BTreeMap::new();

    for file in generated_files {
        let path = file.path.trim().replace('\\', "/");
        if path.is_empty() {
            bail!("technical backlog agent returned a file entry without a path");
        }
        if actual_files
            .insert(path.clone(), file.contents.replace("\r\n", "\n"))
            .is_some()
        {
            bail!("technical backlog agent returned duplicate file `{path}`");
        }
    }

    let actual_paths = actual_files.keys().cloned().collect::<BTreeSet<_>>();
    if actual_paths != expected_paths {
        let missing = expected_paths
            .difference(&actual_paths)
            .cloned()
            .collect::<Vec<_>>();
        let extra = actual_paths
            .difference(&expected_paths)
            .cloned()
            .collect::<Vec<_>>();
        bail!(
            "technical backlog agent returned the wrong file set (missing: {}; extra: {})",
            format_path_list(&missing),
            format_path_list(&extra),
        );
    }

    let rendered_files = template_files
        .iter()
        .map(|template| {
            let contents = actual_files
                .remove(&template.relative_path)
                .ok_or_else(|| {
                    anyhow!(
                        "technical backlog agent omitted `{}`",
                        template.relative_path
                    )
                })?;

            if contents.trim().is_empty() {
                bail!(
                    "technical backlog agent returned empty contents for `{}`",
                    template.relative_path
                );
            }

            Ok(RenderedTemplateFile {
                relative_path: template.relative_path.clone(),
                contents,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    ensure_no_unresolved_placeholders(&rendered_files)?;
    Ok(rendered_files)
}

fn render_technical_session(frame: &mut Frame<'_>, app: &TechnicalSessionApp) {
    match &app.stage {
        TechnicalStage::PickIssue(picker) => {
            render_issue_picker_frame(frame, picker, app.copy.status_text())
        }
        TechnicalStage::SelectCriteria(criteria) => {
            render_acceptance_criteria_frame(frame, criteria, app.copy.status_text())
        }
        TechnicalStage::Questions(questions) => {
            render_questions_frame(frame, questions, app.copy.status_text())
        }
        TechnicalStage::Refinement(refinement) => render_review_refinement_frame(frame, refinement),
        TechnicalStage::Loading(loading) => render_loading_frame(frame, loading),
        TechnicalStage::Review(review) => {
            render_review_frame(frame, review, app.copy.status_text())
        }
    }
    app.copy.render_export_overlay(frame, frame.area());
}

fn render_issue_picker_frame(
    frame: &mut Frame<'_>,
    app: &IssuePickerApp,
    copy_status: Option<&str>,
) {
    let layout = base_layout(frame);
    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(layout[0]);
    let content = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(body[1]);

    let query_block = Block::default()
        .borders(Borders::ALL)
        .title("Select Parent Issue [search]")
        .border_style(Style::default().add_modifier(Modifier::BOLD));
    let query_inner = query_block.inner(body[0]);
    let rendered_query = app.query.render_with_width(
        "Search by identifier, title, state, project, or description...",
        true,
        query_inner.width,
    );
    let query = rendered_query.paragraph(query_block);
    frame.render_widget(query, body[0]);
    rendered_query.set_cursor(frame, query_inner);

    let filtered = search_results(app);
    let mut issue_state = ListState::default();
    issue_state.select(Some(app.selected.min(filtered.len().saturating_sub(1))));
    let issue_items = if filtered.is_empty() {
        vec![ListItem::new("No issues match the current search.")]
    } else {
        filtered
            .iter()
            .filter_map(|result| {
                app.issues
                    .get(result.issue_index)
                    .map(|issue| render_issue_row(issue, Some(result), None))
            })
            .collect::<Vec<_>>()
    };
    let issue_list = List::new(issue_items)
        .block(Block::default().borders(Borders::ALL).title(format!(
            "Issues ({}/{})",
            filtered.len(),
            app.issues.len()
        )))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");
    frame.render_stateful_widget(issue_list, content[0], &mut issue_state);

    let preview = filtered
        .get(app.selected)
        .and_then(|result| {
            app.issues.get(result.issue_index).map(|issue| {
                render_issue_preview(
                    issue,
                    Some(result),
                    None,
                    "_No Linear description was provided._",
                )
            })
        })
        .unwrap_or_else(|| {
            Text::from(vec![
                Line::from("Search results appear here."),
                Line::from(""),
                Line::styled(
                    "Type to narrow the ticket list, then press Enter to generate the technical backlog draft.",
                    Style::default().add_modifier(Modifier::DIM),
                ),
            ])
        });
    let preview = scrollable_content_paragraph(
        preview,
        if app.focus == IssuePickerFocus::Preview {
            "Issue Preview [focus]"
        } else {
            "Issue Preview"
        },
        &app.preview_scroll,
    )
    .wrap(Wrap { trim: false });
    frame.render_widget(preview, content[1]);

    render_footer(
        frame,
        layout[1],
        app.error.as_deref(),
        copy_status,
        &field_copy_help(
            "Type to search issues by identifier, title, state, project, or description. Tab switches between the issue list and preview. Up/Down moves the active pane, and PgUp/PgDn/Home/End or the mouse wheel scroll the preview when focused. Enter generates the technical backlog draft. Esc cancels.",
        ),
    );
}

fn render_acceptance_criteria_frame(
    frame: &mut Frame<'_>,
    app: &AcceptanceCriteriaApp,
    copy_status: Option<&str>,
) {
    let layout = base_layout(frame);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(layout[0]);

    let selected = app.criteria.selected_indices();
    let mut criteria_state = ListState::default();
    criteria_state.select(Some(
        app.criteria
            .cursor()
            .min(app.criteria.options().len().saturating_sub(1)),
    ));
    let criteria_items = if app.criteria.options().is_empty() {
        vec![ListItem::new(
            "No acceptance criteria were found in the issue description.",
        )]
    } else {
        app.criteria
            .options()
            .iter()
            .enumerate()
            .map(|(index, criterion)| {
                let marker = if selected.contains(&index) {
                    "[x]"
                } else {
                    "[ ]"
                };
                ListItem::new(format!("{marker} {criterion}"))
            })
            .collect::<Vec<_>>()
    };
    let criteria_list = List::new(criteria_items)
        .block(Block::default().borders(Borders::ALL).title(format!(
            "Acceptance Criteria ({}/{})",
            selected.len(),
            app.criteria.options().len()
        )))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");
    frame.render_stateful_widget(criteria_list, body[0], &mut criteria_state);

    let mut summary_lines = vec![
        Line::from(format!("Parent: {}", app.parent.identifier)),
        Line::from(app.parent.title.clone()),
        Line::from(""),
        Line::styled(
            "Selected criteria will be carried into the technical prompt alongside the repository scan and planning context.",
            Style::default().add_modifier(Modifier::DIM),
        ),
        Line::from(""),
        Line::from("Selected"),
    ];

    if selected.is_empty() {
        summary_lines.push(Line::styled(
            "_No acceptance criteria selected yet._",
            Style::default().add_modifier(Modifier::DIM),
        ));
    } else {
        for criterion in app.criteria.selected_labels() {
            summary_lines.push(Line::from(format!("- {criterion}")));
        }
    }

    let summary = Paragraph::new(Text::from(summary_lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Selection Summary"),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(summary, body[1]);

    render_footer(
        frame,
        layout[1],
        app.error.as_deref(),
        copy_status,
        &pane_copy_help(
            "Up/Down moves between acceptance criteria. Space toggles each criterion. Enter generates the technical backlog draft from the selected criteria. Esc cancels.",
        ),
    );
}

fn render_questions_frame(
    frame: &mut Frame<'_>,
    app: &TechnicalQuestionsApp,
    copy_status: Option<&str>,
) {
    let layout = base_layout(frame);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(68), Constraint::Percentage(32)])
        .split(layout[0]);
    let main = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(42), Constraint::Min(0)])
        .split(body[0]);

    let question = app.questions.get(app.selected);
    let question_title = if let Some(question) = question {
        match question.state {
            FollowUpAnswerState::Pending => {
                format!("Question {} [pending]", app.selected + 1)
            }
            FollowUpAnswerState::Answered => {
                format!("Question {} [answered]", app.selected + 1)
            }
            FollowUpAnswerState::Skipped => {
                format!("Question {} [skipped]", app.selected + 1)
            }
        }
    } else {
        "Question".to_string()
    };

    let question_text = question
        .map(|question| Text::from(question.question.clone()))
        .unwrap_or_else(|| Text::from("No follow-up questions are pending."));
    let question_panel = Paragraph::new(question_text)
        .block(Block::default().borders(Borders::ALL).title(question_title))
        .wrap(Wrap { trim: false });
    frame.render_widget(question_panel, main[0]);

    let input_block = Block::default().borders(Borders::ALL).title("Answer Draft");
    let input_inner = technical_questions_answer_viewport(frame.area());
    let rendered_answer = question
        .map(|question| {
            question.answer.render_with_viewport(
                "Write the answer for this follow-up question. Leave blank and press Enter to skip.",
                true,
                input_inner.width,
                input_inner.height,
            )
        })
        .unwrap_or_else(|| {
            InputFieldState::multiline(String::new()).render_with_viewport(
                "",
                false,
                input_inner.width,
                input_inner.height,
            )
        });
    frame.render_widget(rendered_answer.paragraph(input_block), main[1]);
    rendered_answer.set_cursor(frame, input_inner);

    let pending_count = app
        .questions
        .len()
        .saturating_sub(app.answered_count() + app.skipped_count());
    let follow_up_lines = if app.workflow.follow_ups.is_empty() {
        vec![Line::styled(
            "_No prior follow-up answers recorded._",
            Style::default().add_modifier(Modifier::DIM),
        )]
    } else {
        app.workflow
            .follow_ups
            .iter()
            .enumerate()
            .flat_map(|(index, response)| {
                let answer = if response.skipped {
                    SKIPPED_TECHNICAL_FOLLOW_UP_LABEL.to_string()
                } else {
                    response.answer.clone()
                };
                [
                    Line::styled(
                        format!("Q{}: {}", index + 1, response.question),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Line::from(answer),
                    Line::from(""),
                ]
            })
            .collect::<Vec<_>>()
    };

    let mut summary_lines = vec![
        Line::from(format!("Parent: {}", app.workflow.parent.identifier)),
        Line::from(app.workflow.parent.title.clone()),
        Line::from(""),
        Line::from(format!(
            "Question {}/{}",
            app.selected
                .saturating_add(1)
                .min(app.questions.len().max(1)),
            app.questions.len()
        )),
        Line::from(format!("Answered: {}", app.answered_count())),
        Line::from(format!("Skipped: {}", app.skipped_count())),
        Line::from(format!("Pending: {pending_count}")),
        Line::from(""),
        Line::styled(
            "Recorded answers",
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ];
    summary_lines.extend(follow_up_lines);

    let summary = Paragraph::new(Text::from(summary_lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Technical Context"),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(summary, body[1]);

    render_footer(
        frame,
        layout[1],
        app.error.as_deref(),
        copy_status,
        &field_copy_help(
            "Tab/Shift+Tab switch questions. Enter records answer and advances. Shift+Enter newline. Ctrl+S submits all answers when complete. Ctrl+V paste. Esc cancel.",
        ),
    );
}

fn render_review_frame(frame: &mut Frame<'_>, app: &TechnicalReviewApp, copy_status: Option<&str>) {
    let layout = base_layout(frame);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(layout[0]);
    let sidebar = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(16), Constraint::Min(0)])
        .split(body[0]);

    let criteria_summary = if app.workflow.selected_acceptance_criteria.is_empty() {
        "Criteria: using the full issue context".to_string()
    } else {
        format!(
            "Criteria: {} selected",
            app.workflow.selected_acceptance_criteria.len()
        )
    };

    let summary = Paragraph::new(Text::from(vec![
        Line::from(format!("Parent: {}", app.workflow.parent.identifier)),
        Line::from(app.workflow.parent.title.clone()),
        Line::from(""),
        Line::from(format!("Child: {}", app.workflow.child_title)),
        Line::from(format!("Files: {}", app.workflow.files.len())),
        Line::from(criteria_summary),
        Line::from(format!("Follow-ups: {}", app.workflow.follow_ups.len())),
        Line::from(format!(
            "Refinements: {}",
            app.workflow.refinement_history.len()
        )),
        Line::from(""),
        Line::styled(
            "Review the latest generated Markdown files before creating the technical child issue.",
            Style::default().add_modifier(Modifier::DIM),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Technical Draft"),
    )
    .wrap(Wrap { trim: false });
    frame.render_widget(summary, sidebar[0]);

    let mut file_state = ListState::default();
    file_state.select(Some(
        app.selected_file
            .min(app.workflow.files.len().saturating_sub(1)),
    ));
    let file_items = app
        .workflow
        .files
        .iter()
        .map(|file| ListItem::new(file.relative_path.clone()))
        .collect::<Vec<_>>();
    let file_list = List::new(file_items)
        .block(Block::default().borders(Borders::ALL).title(
            if app.focus == TechnicalReviewFocus::Files {
                "Generated Files [focus]"
            } else {
                "Generated Files"
            },
        ))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");
    frame.render_stateful_widget(file_list, sidebar[1], &mut file_state);

    let selected_file = &app.workflow.files[app.selected_file];
    let preview = scrollable_paragraph_with_block(
        app.preview_text(),
        Block::default()
            .borders(Borders::ALL)
            .title(if app.focus == TechnicalReviewFocus::Preview {
                format!("Preview: {} [focus]", selected_file.relative_path)
            } else {
                format!("Preview: {}", selected_file.relative_path)
            })
            .border_style(if app.focus == TechnicalReviewFocus::Preview {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            }),
        &app.preview_scroll,
    )
    .wrap(Wrap { trim: false });
    frame.render_widget(preview, body[1]);

    render_footer(
        frame,
        layout[1],
        app.error.as_deref(),
        copy_status,
        &pane_copy_help(
            "Tab/Shift+Tab switch focus. Up/Down move the active pane, and PgUp/PgDn/Home/End or the mouse wheel scroll the preview when focused. F refines. Enter creates the technical child issue and syncs the latest reviewed Markdown files. Esc cancels.",
        ),
    );
}

fn render_review_refinement_frame(frame: &mut Frame<'_>, app: &TechnicalReviewRefinementApp) {
    let layout = base_layout(frame);
    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Min(0)])
        .split(layout[0]);

    let history_text = if app.review.workflow.refinement_history.is_empty() {
        Text::from("No previous refinements. Type new guidance below and press Ctrl+S to rebuild.")
    } else {
        let mut lines = vec![
            Line::from(format!("Parent: {}", app.review.workflow.parent.identifier)),
            Line::from(app.review.workflow.parent.title.clone()),
            Line::from(""),
            Line::styled(
                format!(
                    "Previous refinements ({})",
                    app.review.workflow.refinement_history.len()
                ),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ];
        for (index, addendum) in app.review.workflow.refinement_history.iter().enumerate() {
            lines.push(Line::from(""));
            lines.push(Line::styled(
                format!("#{}", index + 1),
                Style::default().add_modifier(Modifier::BOLD),
            ));
            lines.push(Line::from(addendum.clone()));
        }
        Text::from(lines)
    };
    let history = Paragraph::new(history_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Refinement History"),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(history, body[0]);

    let input_block = Block::default()
        .borders(Borders::ALL)
        .title("Refinement Guidance");
    let input_inner = technical_refinement_input_viewport(frame.area());
    let rendered = app.addendum.render_with_viewport(
        "Describe what should change in the next technical draft...",
        true,
        input_inner.width,
        input_inner.height,
    );
    frame.render_widget(rendered.paragraph(input_block), body[1]);
    rendered.set_cursor(frame, input_inner);

    render_footer(
        frame,
        layout[1],
        app.error.as_deref(),
        None,
        "Type the next refinement guidance. Ctrl+S rebuilds the draft. Enter inserts a newline. Esc returns to the review screen.",
    );
}

fn render_loading_frame(frame: &mut Frame<'_>, app: &LoadingApp) {
    render_loading_panel(
        frame,
        frame.area(),
        &LoadingPanelData {
            title: "Agent Working [loading]".to_string(),
            message: app.message.clone(),
            detail: app.detail.clone(),
            spinner_index: app.spinner_index,
            status_line: agent_loading_status_line().to_string(),
        },
    );
}

fn search_results(app: &IssuePickerApp) -> Vec<IssueSearchResult> {
    search_issues(&app.issues, app.query.value().trim())
}

fn base_layout(frame: &mut Frame<'_>) -> Vec<Rect> {
    base_layout_for_area(frame.area())
}

fn base_layout_for_area(area: Rect) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(5)])
        .split(area)
        .to_vec()
}

fn inner_rect(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2).max(1),
        area.height.saturating_sub(2).max(1),
    )
}

fn render_footer(
    frame: &mut Frame<'_>,
    area: Rect,
    error: Option<&str>,
    status: Option<&str>,
    help: &str,
) {
    let mut lines = vec![Line::from(help.to_string())];
    if let Some(message) = error.or(status) {
        lines.push(Line::from(""));
        lines.push(Line::styled(
            message.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ));
    }
    let footer = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::ALL).title("Controls"))
        .wrap(Wrap { trim: false });
    frame.render_widget(footer, area);
}

fn current_local_date() -> Result<String> {
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    OffsetDateTime::now_utc()
        .to_offset(offset)
        .format(&format_description!("[year]-[month]-[day]"))
        .context("failed to format the current date for the technical backlog prompt")
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;

    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash && !slug.is_empty() {
            slug.push('-');
            last_was_dash = true;
        }
    }

    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "technical-item".to_string()
    } else {
        slug
    }
}

fn load_context_bundle(root: &Path) -> Result<String> {
    load_codebase_context_bundle(
        root,
        &[
            CodebaseContextSection::Scan,
            CodebaseContextSection::Architecture,
            CodebaseContextSection::Concerns,
            CodebaseContextSection::Conventions,
            CodebaseContextSection::Integrations,
            CodebaseContextSection::Stack,
            CodebaseContextSection::Structure,
            CodebaseContextSection::Testing,
        ],
        MissingCodebaseContextHint::Scan,
    )
}

fn render_repository_snapshot(root: &Path) -> Result<String> {
    let mut lines = Vec::new();
    let mut remaining = 80usize;
    collect_directory_snapshot(root, root, 0, 2, &mut remaining, &mut lines)?;

    if lines.is_empty() {
        Ok("_Repository snapshot is empty._".to_string())
    } else if remaining == 0 {
        lines.push("... (truncated)".to_string());
        Ok(lines.join("\n"))
    } else {
        Ok(lines.join("\n"))
    }
}

fn collect_directory_snapshot(
    root: &Path,
    current: &Path,
    depth: usize,
    max_depth: usize,
    remaining: &mut usize,
    lines: &mut Vec<String>,
) -> Result<()> {
    if *remaining == 0 || depth > max_depth {
        return Ok(());
    }

    let mut entries = fs::read_dir(current)
        .with_context(|| format!("failed to read directory `{}`", current.display()))?
        .filter_map(|entry| entry.ok())
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        if *remaining == 0 {
            break;
        }

        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if should_skip_snapshot_entry(&file_name) {
            continue;
        }

        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect `{}`", path.display()))?;
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let indent = "  ".repeat(depth);
        let display = if file_type.is_dir() {
            format!("{indent}- {}/", relative.display())
        } else {
            format!("{indent}- {}", relative.display())
        };
        lines.push(display);
        *remaining = remaining.saturating_sub(1);

        if file_type.is_dir() {
            collect_directory_snapshot(root, &path, depth + 1, max_depth, remaining, lines)?;
        }
    }

    Ok(())
}

fn should_skip_snapshot_entry(name: &str) -> bool {
    matches!(
        name,
        ".git" | "target" | "node_modules" | ".next" | "dist" | "build" | "coverage"
    )
}

fn parse_agent_json<T>(raw: &str, phase: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let trimmed = raw.trim();
    for candidate in parse_json_candidates(trimmed) {
        if let Ok(parsed) = serde_json::from_str::<T>(&candidate) {
            return Ok(parsed);
        }
    }

    eprintln!(
        "warning: technical backlog JSON parse failed during {phase}; raw agent output:\n{trimmed}"
    );
    bail!(
        "technical backlog agent returned invalid JSON during {phase}: {}",
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
    if value.len() <= MAX_PREVIEW_LEN {
        value.to_string()
    } else {
        format!("{}...", &value[..MAX_PREVIEW_LEN])
    }
}

fn format_path_list(paths: &[String]) -> String {
    if paths.is_empty() {
        "none".to_string()
    } else {
        paths.join(", ")
    }
}

fn extract_acceptance_criteria(description: Option<&str>) -> Vec<String> {
    let Some(description) = description else {
        return Vec::new();
    };

    let mut in_acceptance_criteria = false;
    let mut current_item = None::<String>;
    let mut items = Vec::new();

    for line in description.lines() {
        let trimmed = line.trim();

        if is_markdown_header(trimmed) {
            let is_acceptance_header = header_title(trimmed)
                .map(|title| {
                    title
                        .trim_end_matches(':')
                        .eq_ignore_ascii_case("acceptance criteria")
                })
                .unwrap_or(false);

            if in_acceptance_criteria && !is_acceptance_header {
                if let Some(item) = current_item.take()
                    && !item.trim().is_empty()
                {
                    items.push(item);
                }
                break;
            }

            in_acceptance_criteria = is_acceptance_header;
            continue;
        }

        if !in_acceptance_criteria {
            continue;
        }

        if let Some(item) = parse_markdown_list_item(trimmed) {
            if let Some(previous) = current_item.replace(item)
                && !previous.trim().is_empty()
            {
                items.push(previous);
            }
            continue;
        }

        if trimmed.is_empty() {
            if let Some(previous) = current_item.take()
                && !previous.trim().is_empty()
            {
                items.push(previous);
            }
            continue;
        }

        if let Some(existing) = current_item.as_mut() {
            existing.push(' ');
            existing.push_str(trimmed);
        }
    }

    if let Some(item) = current_item
        && !item.trim().is_empty()
    {
        items.push(item);
    }

    items
}

fn is_markdown_header(line: &str) -> bool {
    line.starts_with('#')
}

fn header_title(line: &str) -> Option<&str> {
    let trimmed = line.trim_start_matches('#').trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn parse_markdown_list_item(line: &str) -> Option<String> {
    let stripped = line.trim_start();
    for prefix in ["- [ ] ", "- [x] ", "- [X] ", "* [ ] ", "* [x] ", "* [X] "] {
        if let Some(rest) = stripped.strip_prefix(prefix) {
            return normalized_list_item(rest);
        }
    }

    for prefix in ["- ", "* ", "+ "] {
        if let Some(rest) = stripped.strip_prefix(prefix) {
            return normalized_list_item(rest);
        }
    }

    let digit_count = stripped
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .count();
    if digit_count > 0 {
        let remainder = &stripped[digit_count..];
        if let Some(rest) = remainder
            .strip_prefix(". ")
            .or_else(|| remainder.strip_prefix(") "))
        {
            return normalized_list_item(rest);
        }
    }

    None
}

fn normalized_list_item(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

struct TerminalCleanup;

impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(
            stdout,
            DisableMouseCapture,
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
    }
}

fn issue_picker_preview_viewport(area: Rect) -> Rect {
    let layout = base_layout_for_area(area);
    let content = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(layout[0]);
    inner_rect(content[1])
}

fn technical_questions_answer_viewport(area: Rect) -> Rect {
    let layout = base_layout_for_area(area);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(68), Constraint::Percentage(32)])
        .split(layout[0]);
    let main = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(42), Constraint::Min(0)])
        .split(body[0]);
    inner_rect(main[1])
}

fn technical_refinement_input_viewport(area: Rect) -> Rect {
    let layout = base_layout_for_area(area);
    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Min(0)])
        .split(layout[0]);
    inner_rect(body[1])
}

fn technical_review_preview_viewport(area: Rect) -> Rect {
    let layout = base_layout_for_area(area);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(layout[0]);
    inner_rect(body[1])
}

#[cfg(test)]
fn snapshot(backend: &TestBackend) -> String {
    let buffer = backend.buffer();
    let mut lines = Vec::new();

    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{
        AcceptanceCriteriaApp, FollowUpAnswerState, IssuePickerApp, IssuePickerFocus, LoadingApp,
        ParsedTechnicalRouteResponse, PendingTechnicalJob, QuestionAnswer, TechnicalAction,
        TechnicalPromptKind, TechnicalQuestionsApp, TechnicalReviewApp, TechnicalReviewFocus,
        TechnicalReviewRefinementApp, TechnicalSessionApp, TechnicalStage, TechnicalWorkerReport,
        TechnicalWorkflowState, build_review_refinement_app, extract_acceptance_criteria,
        handle_issue_picker_key, handle_issue_picker_paste, parse_agent_json,
        parse_technical_route_response, process_pending_generation,
        render_acceptance_criteria_frame, render_issue_picker_frame, render_loading_frame,
        render_questions_frame, render_review_frame, render_review_refinement_frame,
        render_technical_prompt, search_results, slugify, snapshot,
        validate_non_interactive_answer_count,
    };
    use crate::backlog::RenderedTemplateFile;
    use crate::fs::PlanningPaths;
    use crate::linear::{
        IssueSummary, ProjectRef, TeamRef, TicketDiscussionBudgets, WorkflowState,
        prepare_issue_context,
    };
    use crate::tui::copy::CopyUiState;
    use crate::tui::fields::{InputFieldState, MultiSelectFieldState};
    use crate::tui::scroll::ScrollState;
    use anyhow::anyhow;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use std::fs;
    use std::sync::mpsc;
    use tempfile::tempdir;

    fn issue(identifier: &str, title: &str, description: &str) -> IssueSummary {
        IssueSummary {
            id: format!("id-{identifier}"),
            identifier: identifier.to_string(),
            title: title.to_string(),
            description: Some(description.to_string()),
            url: format!("https://linear.app/{identifier}"),
            priority: Some(2),
            estimate: None,
            updated_at: "2026-03-14T12:00:00Z".to_string(),
            team: TeamRef {
                id: "team-1".to_string(),
                key: "MET".to_string(),
                name: "Metastack".to_string(),
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
                name: "Todo".to_string(),
                kind: Some("unstarted".to_string()),
            }),
            attachments: Vec::new(),
            parent: None,
            children: Vec::new(),
        }
    }

    fn rendered_file(path: &str, contents: &str) -> RenderedTemplateFile {
        RenderedTemplateFile {
            relative_path: path.to_string(),
            contents: contents.to_string(),
        }
    }

    fn workflow_with_files(
        title: &str,
        description: &str,
        selected_acceptance_criteria: Vec<String>,
        template_files: Vec<RenderedTemplateFile>,
        files: Vec<RenderedTemplateFile>,
    ) -> TechnicalWorkflowState {
        let parent = issue("MET-35", title, description);
        let child_title = format!("Technical: {title}");
        TechnicalWorkflowState {
            parent: parent.clone(),
            child_title: child_title.clone(),
            selected_acceptance_criteria,
            prepared_context: prepare_issue_context(&parent, TicketDiscussionBudgets::default()),
            template_files,
            backlog_slug: slugify(&child_title),
            today: "2026-03-14".to_string(),
            follow_ups: Vec::new(),
            questions_asked: 0,
            refinement_history: Vec::new(),
            files,
            revision: 1,
        }
    }

    fn render_picker_snapshot(app: &IssuePickerApp) -> String {
        let backend = TestBackend::new(140, 36);
        let mut terminal = Terminal::new(backend).expect("terminal should initialize");
        terminal
            .draw(|frame| render_issue_picker_frame(frame, app, None))
            .expect("picker should render");
        snapshot(terminal.backend())
    }

    fn render_review_snapshot(app: &TechnicalReviewApp) -> String {
        let backend = TestBackend::new(140, 36);
        let mut terminal = Terminal::new(backend).expect("terminal should initialize");
        terminal
            .draw(|frame| render_review_frame(frame, app, None))
            .expect("review should render");
        snapshot(terminal.backend())
    }

    fn render_criteria_snapshot(app: &AcceptanceCriteriaApp) -> String {
        let backend = TestBackend::new(140, 36);
        let mut terminal = Terminal::new(backend).expect("terminal should initialize");
        terminal
            .draw(|frame| render_acceptance_criteria_frame(frame, app, None))
            .expect("criteria selector should render");
        snapshot(terminal.backend())
    }

    fn render_questions_snapshot(app: &TechnicalQuestionsApp) -> String {
        let backend = TestBackend::new(140, 36);
        let mut terminal = Terminal::new(backend).expect("terminal should initialize");
        terminal
            .draw(|frame| render_questions_frame(frame, app, None))
            .expect("questions should render");
        snapshot(terminal.backend())
    }

    fn render_loading_snapshot(app: &LoadingApp) -> String {
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).expect("terminal should initialize");
        terminal
            .draw(|frame| render_loading_frame(frame, app))
            .expect("loading should render");
        snapshot(terminal.backend())
    }

    fn render_refinement_snapshot(app: &TechnicalReviewRefinementApp) -> String {
        let backend = TestBackend::new(140, 36);
        let mut terminal = Terminal::new(backend).expect("terminal should initialize");
        terminal
            .draw(|frame| render_review_refinement_frame(frame, app))
            .expect("refinement should render");
        snapshot(terminal.backend())
    }

    #[test]
    fn picker_search_prefers_identifier_and_title_matches() {
        let picker = IssuePickerApp {
            query: InputFieldState::new("met-42 terminal"),
            issues: vec![
                issue("MET-12", "Cleanup docs", "Documentation cleanup"),
                issue("MET-42", "Terminal experience", "Improve terminal flow"),
            ],
            selected: 0,
            focus: IssuePickerFocus::List,
            preview_scroll: ScrollState::default(),
            error: None,
            sticky_error: false,
        };

        let filtered = search_results(&picker);
        assert_eq!(
            filtered
                .iter()
                .map(|result| result.issue_index)
                .collect::<Vec<_>>(),
            vec![1]
        );
    }

    #[test]
    fn issue_picker_snapshot_shows_search_and_preview() {
        let snapshot = render_picker_snapshot(&IssuePickerApp {
            query: InputFieldState::new("terminal"),
            issues: vec![
                issue(
                    "MET-42",
                    "Terminal experience",
                    "Improve the terminal planning flow.",
                ),
                issue("MET-43", "Sync polish", "Refine sync previews."),
            ],
            selected: 0,
            focus: IssuePickerFocus::List,
            preview_scroll: ScrollState::default(),
            error: None,
            sticky_error: false,
        });

        assert!(snapshot.contains("Select Parent Issue [search]"));
        assert!(snapshot.contains("MET-42  Terminal experience"));
        assert!(snapshot.contains("Issue Preview"));
        assert!(snapshot.contains("mouse wheel scroll the preview"));
    }

    #[test]
    fn issue_picker_paste_updates_the_search_query() {
        let mut app = IssuePickerApp {
            query: InputFieldState::new("tech"),
            issues: vec![issue(
                "MET-42",
                "Terminal experience",
                "Improve planning flow.",
            )],
            selected: 1,
            focus: IssuePickerFocus::List,
            preview_scroll: ScrollState::default(),
            error: Some("stale".to_string()),
            sticky_error: false,
        };

        handle_issue_picker_paste(&mut app, " backlog\n generator\n");

        assert_eq!(app.query.value(), "tech backlog generator");
        assert_eq!(app.selected, 0);
        assert_eq!(app.error, None);
    }

    #[test]
    fn review_snapshot_lists_generated_files_and_preview() {
        let workflow = workflow_with_files(
            "Create the technical command",
            "Parent description",
            vec![
                "The command generates backlog docs".to_string(),
                "The docs stay in sync".to_string(),
            ],
            vec![
                rendered_file("index.md", "# Technical draft"),
                rendered_file("specification.md", "# Specification"),
            ],
            vec![
                rendered_file("index.md", "# Technical draft"),
                rendered_file("specification.md", "# Specification"),
            ],
        );
        let snapshot = render_review_snapshot(&TechnicalReviewApp {
            workflow,
            selected_file: 1,
            focus: TechnicalReviewFocus::Files,
            preview_scroll: ScrollState::default(),
            error: None,
        });

        assert!(snapshot.contains("Technical Draft"));
        assert!(snapshot.contains("Generated Files"));
        assert!(snapshot.contains("Preview: specification.md"));
        assert!(snapshot.contains("Criteria: 2 selected"));
    }

    #[test]
    fn review_preview_scrolls_to_bottom_of_long_file() {
        let workflow = workflow_with_files(
            "Create the technical command",
            "Parent description",
            vec!["The docs stay in sync".to_string()],
            vec![rendered_file("specification.md", "# Specification")],
            vec![rendered_file(
                "specification.md",
                &(1..=60)
                    .map(|index| format!("technical preview line {index}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )],
        );
        let mut app = TechnicalReviewApp {
            workflow,
            selected_file: 0,
            focus: TechnicalReviewFocus::Preview,
            preview_scroll: ScrollState::default(),
            error: None,
        };

        let _ = app.preview_scroll.apply_key_code_in_viewport(
            crossterm::event::KeyCode::End,
            Rect::new(0, 0, 70, 20),
            app.preview_content_rows(70),
        );

        assert!(app.preview_scroll.offset() > 0);
    }

    #[test]
    fn loading_snapshot_matches_plan_style() {
        let snapshot = render_loading_snapshot(&LoadingApp {
            message: "Generating technical backlog".to_string(),
            detail: format!(
                "Building `{}/backlog/_TEMPLATE` for MET-35.",
                crate::branding::PROJECT_DIR
            ),
            spinner_index: 2,
        });

        assert!(snapshot.contains("[=== ] Generating technical backlog"));
        assert!(snapshot.contains("Agent Working [loading]"));
    }

    #[test]
    fn issue_picker_snapshot_shows_zero_results_state() {
        let snapshot = render_picker_snapshot(&IssuePickerApp {
            query: InputFieldState::new("zzz"),
            issues: vec![issue(
                "MET-42",
                "Terminal experience",
                "Improve planning flow.",
            )],
            selected: 0,
            focus: IssuePickerFocus::List,
            preview_scroll: ScrollState::default(),
            error: None,
            sticky_error: false,
        });

        assert!(snapshot.contains("No issues match the current search."));
        assert!(snapshot.contains("Search results appear here."));
    }

    #[test]
    fn parse_agent_json_accepts_progressive_brace_scan() {
        let parsed: serde_json::Value = parse_agent_json(
            "Context {not json}\n{\"files\":[{\"path\":\"index.md\",\"contents\":\"# Draft\"}]}",
            "technical backlog generation",
        )
        .expect("progressive brace scan should find the JSON payload");

        assert_eq!(parsed["files"][0]["path"], "index.md");
    }

    #[test]
    fn parse_technical_route_response_accepts_tagged_questions() {
        let parsed = parse_technical_route_response(
            "{\"kind\":\"questions\",\"questions\":[\"Which repo area is in scope?\"]}",
            "technical backlog generation",
        )
        .expect("tagged question response should parse");

        match parsed {
            ParsedTechnicalRouteResponse::Questions { questions } => {
                assert_eq!(questions, vec!["Which repo area is in scope?".to_string()]);
            }
            ParsedTechnicalRouteResponse::Draft { .. } => {
                panic!("expected tagged questions response");
            }
        }
    }

    #[test]
    fn parse_technical_route_response_rejects_missing_kind() {
        let error = parse_technical_route_response(
            "{\"files\":[{\"path\":\"index.md\",\"contents\":\"# Draft\"}]}",
            "technical backlog generation",
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("technical backlog agent response missing string `kind`")
        );
    }

    #[test]
    fn validate_non_interactive_answer_count_rejects_mismatches() {
        assert_eq!(
            validate_non_interactive_answer_count(1, 0)
                .unwrap_err()
                .to_string(),
            "technical agent requested no follow-up questions; remove the provided `--answer` values"
        );
        assert_eq!(
            validate_non_interactive_answer_count(1, 2)
                .unwrap_err()
                .to_string(),
            "technical agent requested 2 follow-up question(s); pass exactly 2 `--answer` value(s)"
        );
    }

    #[test]
    fn picker_keeps_sticky_recovered_error_visible_during_navigation() {
        let mut app = IssuePickerApp {
            query: InputFieldState::new("MET"),
            issues: vec![issue(
                "MET-42",
                "Terminal experience",
                "Improve planning flow.",
            )],
            selected: 0,
            focus: IssuePickerFocus::List,
            preview_scroll: ScrollState::default(),
            error: Some("recovered".to_string()),
            sticky_error: true,
        };

        let action = handle_issue_picker_key(
            &mut app,
            &mut CopyUiState::default(),
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Tab,
                crossterm::event::KeyModifiers::NONE,
            ),
            Rect::new(0, 0, 80, 20),
        );

        assert!(matches!(action, TechnicalAction::None));
        assert_eq!(app.error.as_deref(), Some("recovered"));
        assert!(app.sticky_error);
    }

    #[test]
    fn process_pending_generation_restores_picker_error_as_sticky() {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(TechnicalWorkerReport {
                continuation: None,
                outcome: Err(anyhow!("recovered failure")),
            })
            .expect("generation failure should send");
        drop(sender);

        let picker = IssuePickerApp {
            query: InputFieldState::new("MET"),
            issues: vec![issue(
                "MET-42",
                "Terminal experience",
                "Improve planning flow.",
            )],
            selected: 0,
            focus: IssuePickerFocus::List,
            preview_scroll: ScrollState::default(),
            error: None,
            sticky_error: false,
        };
        let mut app = TechnicalSessionApp {
            stage: TechnicalStage::Loading(LoadingApp {
                message: "Generating technical backlog".to_string(),
                detail: "Building backlog files.".to_string(),
                spinner_index: 0,
            }),
            copy: CopyUiState::default(),
            agent_overrides: super::TechnicalAgentOverrides::default(),
            continuation: None,
            question_limit: 4,
            refinement_round_limit: 2,
            pending: Some(PendingTechnicalJob {
                receiver,
                previous_stage: Some(TechnicalStage::PickIssue(picker)),
            }),
        };

        process_pending_generation(&mut app).expect("pending generation should restore the picker");

        match app.stage {
            TechnicalStage::PickIssue(restored) => {
                assert_eq!(restored.error.as_deref(), Some("recovered failure"));
                assert!(restored.sticky_error);
            }
            other => panic!("expected picker stage, got {other:?}"),
        }
    }

    #[test]
    fn acceptance_criteria_parser_collects_markdown_list_items() {
        let description = r#"
# Context
Some setup.

## Acceptance Criteria
- [ ] Script exists in the repository root
- [x] Script exits cleanly on interruption
1. Usage is documented
   with a wrapped continuation line

## Notes
Ignored.
"#;

        assert_eq!(
            extract_acceptance_criteria(Some(description)),
            vec![
                "Script exists in the repository root".to_string(),
                "Script exits cleanly on interruption".to_string(),
                "Usage is documented with a wrapped continuation line".to_string(),
            ]
        );
    }

    #[test]
    fn acceptance_criteria_selector_snapshot_shows_selected_items() {
        let snapshot = render_criteria_snapshot(&AcceptanceCriteriaApp {
            parent: issue(
                "MET-56",
                "Create a Merry Christmas script",
                "## Acceptance Criteria\n- festive scene\n- graceful exit",
            ),
            criteria: MultiSelectFieldState::new(
                vec!["festive scene".to_string(), "graceful exit".to_string()],
                [0usize],
            ),
            error: None,
            sticky_error: false,
        });

        assert!(snapshot.contains("Acceptance Criteria (1/2)"));
        assert!(snapshot.contains("[x] festive scene"));
        assert!(snapshot.contains("Selection Summary"));
    }

    #[test]
    fn questions_snapshot_shows_existing_answers_and_controls() {
        let mut workflow = workflow_with_files(
            "Create the technical command",
            "Parent description",
            vec!["Render docs".to_string()],
            vec![rendered_file("index.md", "# Template")],
            vec![rendered_file("index.md", "# Draft")],
        );
        workflow.follow_ups.push(super::TechnicalFollowUpResponse {
            question: "Which repo area already owns the sync path?".to_string(),
            answer: "src/sync.rs".to_string(),
            skipped: false,
        });
        let app = TechnicalQuestionsApp {
            workflow,
            questions: vec![
                QuestionAnswer {
                    question: "Should this draft preserve the current packet layout?".to_string(),
                    answer: InputFieldState::multiline("Yes, keep the existing review layout."),
                    state: FollowUpAnswerState::Answered,
                },
                QuestionAnswer {
                    question: "Do we need a deterministic mismatch error?".to_string(),
                    answer: InputFieldState::multiline(String::new()),
                    state: FollowUpAnswerState::Pending,
                },
            ],
            selected: 1,
            error: None,
            sticky_error: false,
        };

        let snapshot = render_questions_snapshot(&app);
        assert!(snapshot.contains("Question 2 [pending]"));
        assert!(snapshot.contains("Technical Context"));
        assert!(snapshot.contains("Recorded answers"));
        assert!(snapshot.contains("Ctrl+S submits all answers when complete"));
    }

    #[test]
    fn refinement_snapshot_shows_history_and_input() {
        let mut workflow = workflow_with_files(
            "Create the technical command",
            "Parent description",
            vec!["Render docs".to_string()],
            vec![rendered_file("index.md", "# Template")],
            vec![rendered_file("index.md", "# Draft")],
        );
        workflow.refinement_history = vec![
            "Keep the existing review layout and add refine.".to_string(),
            "Ask more questions when the draft is underspecified.".to_string(),
        ];
        let app = build_review_refinement_app(TechnicalReviewApp {
            workflow,
            selected_file: 0,
            focus: TechnicalReviewFocus::Files,
            preview_scroll: ScrollState::default(),
            error: None,
        });

        let snapshot = render_refinement_snapshot(&app);
        assert!(snapshot.contains("Refinement History"));
        assert!(snapshot.contains("Previous refinements (2)"));
        assert!(snapshot.contains("Refinement Guidance"));
        assert!(snapshot.contains("Ctrl+S rebuilds the draft"));
    }

    #[test]
    fn technical_prompt_includes_selected_criteria_and_repo_snapshot() {
        let temp = tempdir().expect("tempdir should be created");
        let root = temp.path();
        let paths = PlanningPaths::new(root);
        fs::create_dir_all(&paths.codebase_dir).expect("codebase dir should be created");
        fs::create_dir_all(root.join("src")).expect("src dir should be created");
        fs::write(paths.scan_path(), "# Scan\nCLI layout").expect("scan context should be written");
        fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("repo file should be written");
        let workflow = workflow_with_files(
            "Create the technical command",
            "## Acceptance Criteria\n- Render docs\n- Keep sync safe",
            vec!["Render docs".to_string(), "Keep sync safe".to_string()],
            vec![rendered_file("index.md", "# {{BACKLOG_TITLE}}")],
            Vec::new(),
        );

        let prompt = render_technical_prompt(root, &workflow, &TechnicalPromptKind::Initial, 2)
            .expect("prompt should render");

        assert!(prompt.contains("Selected acceptance criteria for this technical sub-ticket"));
        assert!(prompt.contains("- Render docs"));
        assert!(prompt.contains("Injected workflow contract:"));
        assert!(prompt.contains("## Built-in Workflow Contract"));
        assert!(
            prompt
                .contains("create backlog content only for work inside this repository directory")
        );
        assert!(prompt.contains("Repository directory snapshot"));
        assert!(prompt.contains("- src/"));
        assert!(prompt.contains("- src/main.rs"));
        assert!(prompt.contains("## SCAN.md"));
        assert!(
            prompt
                .contains("{\"kind\":\"questions\",\"questions\":[\"Question 1\",\"Question 2\"]}")
        );
        assert!(prompt.contains(
            "{\"kind\":\"draft\",\"files\":[{\"path\":\"index.md\",\"contents\":\"# ...\"}]}"
        ));
        assert!(!prompt.contains("MetaStack CLI"));
    }
}

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
    Event, KeyCode, KeyEvent, KeyEventKind, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use serde::{Deserialize, Serialize};

use crate::agents::{AgentContinuation, run_agent_capture_with_continuation};
use crate::backlog::{
    BacklogIssueMetadata, INDEX_FILE_NAME, ManagedFileRecord, RenderedTemplateFile,
    TemplateContext, ensure_no_unresolved_placeholders, render_template_files, save_issue_metadata,
    write_rendered_backlog_item,
};
use crate::backlog_defaults::{
    PlanTicketResolutionInput, RememberedBacklogSelection, TicketOptionOverrides,
    load_remembered_backlog_selection, resolve_plan_ticket_defaults,
    save_remembered_backlog_selection,
};
use crate::branding;
use crate::cli::{RunAgentArgs, SplitArgs, SplitReviewEventArg};
use crate::codebase_context::{
    CodebaseContextSection, MissingCodebaseContextHint, load_codebase_context_bundle,
};
use crate::config::{AGENT_ROUTE_BACKLOG_SPLIT, AppConfig, load_required_planning_meta};
use crate::context::load_compact_workflow_contract;
use crate::fs::{canonicalize_existing_dir, display_path};
use crate::linear::{
    IssueCreateSpec, IssueEditSpec, IssueSummary, PreparedIssueContext, TicketDiscussionBudgets,
    prepare_issue_context, render_ticket_image_summary,
};
use crate::output::{MachineIssueSummary, render_json_success};
use crate::progress::{
    LoadingPanelData, SPINNER_FRAMES, agent_loading_status_line, render_loading_panel,
};
use crate::scaffold::{ensure_backlog_templates, ensure_planning_layout};
use crate::tui::copy::{
    CopyPayload, CopyUiState, copy_overlay_viewport, field_copy_help, pane_copy_help,
};
use crate::tui::fields::{InputFieldState, MultiSelectFieldState};
use crate::tui::keybindings::{is_copy_key, is_mouse_toggle_key, top_level_cancel};
use crate::tui::markdown::render_markdown;
use crate::tui::scroll::{ScrollState, plain_text, scrollable_content_paragraph, wrapped_rows};
use crate::{LinearCommandContext, load_linear_command_context};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct SplitProposal {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    child_issues: Vec<SplitChildDraft>,
    parent_rewrite: SplitParentDraft,
    #[serde(default)]
    dependency_suggestions: Vec<SplitDependencySuggestion>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SplitChildDraft {
    proposal_id: String,
    title: String,
    description: String,
    #[serde(default)]
    acceptance_criteria: Vec<String>,
    #[serde(default)]
    priority: Option<u8>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SplitParentDraft {
    title: String,
    description: String,
    #[serde(default)]
    acceptance_criteria: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SplitDependencySuggestion {
    blocking: String,
    blocked: String,
    #[serde(default)]
    rationale: String,
}

#[derive(Debug, Serialize)]
struct SplitProposalResult<'a> {
    source_issue: MachineIssueSummary,
    summary: &'a str,
    child_issues: &'a [SplitChildDraft],
    parent_rewrite: &'a SplitParentDraft,
    dependency_suggestions: &'a [SplitDependencySuggestion],
    applied: bool,
}

#[derive(Debug)]
pub(crate) struct ProposedSplitReport {
    source: IssueSummary,
    proposal: SplitProposal,
}

#[derive(Debug)]
pub(crate) struct AppliedSplitReport {
    source: IssueSummary,
    parent: IssueSummary,
    children: Vec<IssueSummary>,
    backlog_paths: Vec<String>,
    dependency_links_created: usize,
    dependency_link_notes: Vec<String>,
}

#[derive(Debug)]
pub(crate) enum SplitReport {
    Cancelled,
    Rendered(String),
    Proposed(Box<ProposedSplitReport>),
    Applied(Box<AppliedSplitReport>),
}

impl SplitReport {
    pub(crate) fn render(&self) -> String {
        match self {
            Self::Cancelled => "Backlog split cancelled.".to_string(),
            Self::Rendered(snapshot) => snapshot.clone(),
            Self::Proposed(report) => format!(
                "Generated a split proposal for {} with {} child issue(s).",
                report.source.identifier,
                report.proposal.child_issues.len()
            ),
            Self::Applied(report) => {
                let mut lines = vec![format!(
                    "Split {} into {} child issue(s); parent rewritten as {} and {} dependency link(s) created.",
                    report.source.identifier,
                    report.children.len(),
                    report.parent.identifier,
                    report.dependency_links_created
                )];
                if !report.backlog_paths.is_empty() {
                    lines.push("Backlog packets:".to_string());
                    lines.extend(report.backlog_paths.iter().map(|path| format!("- {path}")));
                }
                if !report.dependency_link_notes.is_empty() {
                    lines.push("Dependency notes:".to_string());
                    lines.extend(
                        report
                            .dependency_link_notes
                            .iter()
                            .map(|note| format!("- {note}")),
                    );
                }
                lines.join("\n")
            }
        }
    }

    /// Render the proposal-mode split result in the standard machine-readable success envelope.
    pub(crate) fn render_json(&self) -> Result<String> {
        match self {
            Self::Proposed(report) => render_json_success(
                "backlog.split",
                &SplitProposalResult {
                    source_issue: MachineIssueSummary::from(&report.source),
                    summary: report.proposal.summary.as_str(),
                    child_issues: &report.proposal.child_issues,
                    parent_rewrite: &report.proposal.parent_rewrite,
                    dependency_suggestions: &report.proposal.dependency_suggestions,
                    applied: false,
                },
            ),
            _ => bail!("split JSON output is only available for `--no-interactive` proposal runs"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitStage {
    Source,
    Children,
    Dependencies,
    Addendum,
    Confirm,
}

struct SplitApp {
    source: IssueSummary,
    proposal: SplitProposal,
    selected_children: MultiSelectFieldState,
    stage: SplitStage,
    selected_dependencies: MultiSelectFieldState,
    preview_scroll: ScrollState,
    addendum: InputFieldState,
    error: Option<String>,
    sticky_error: bool,
    copy: CopyUiState,
    continuation: Option<AgentContinuation>,
    loading: Option<LoadingApp>,
    pending: Option<PendingSplitJob>,
}

#[derive(Debug, Clone)]
struct LoadingApp {
    message: String,
    detail: String,
    spinner_index: usize,
}

struct PendingSplitJob {
    receiver: Receiver<SplitWorkerReport>,
    mode: PendingSplitMode,
}

struct SplitWorkerReport {
    continuation: Option<AgentContinuation>,
    outcome: Result<SplitProposal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingSplitMode {
    InitialGeneration,
    Refinement,
}

#[derive(Debug)]
enum InteractiveSplitExit {
    Cancelled,
    Confirmed {
        proposal: SplitProposal,
        selected_indices: Vec<usize>,
        selected_dependency_indices: Vec<usize>,
    },
}

enum RenderOnceSplitResult {
    Snapshot(String),
    Confirmed {
        proposal: SplitProposal,
        selected_indices: Vec<usize>,
        selected_dependency_indices: Vec<usize>,
    },
}

struct SplitApplyContext<'a, C> {
    root: &'a Path,
    app_config: &'a AppConfig,
    planning_meta: &'a crate::config::PlanningMeta,
    remembered_selection: &'a RememberedBacklogSelection,
    service: &'a crate::linear::LinearService<C>,
    source: &'a IssueSummary,
    args: &'a SplitArgs,
}

type DependencyKey = (String, String);
type DependencySelectionState = (BTreeSet<DependencyKey>, BTreeSet<DependencyKey>);

/// Generate or apply a full inverse-planning split for an existing Linear issue.
///
/// Returns an error when repo planning metadata is missing, the source issue cannot be loaded,
/// the split proposal JSON is invalid, child creation fails, the parent rewrite fails, or local
/// backlog packets cannot be written.
pub async fn run_split(args: &SplitArgs) -> Result<SplitReport> {
    let root = canonicalize_existing_dir(&args.client.root)?;
    let app_config = AppConfig::load()?;
    let planning_meta = load_required_planning_meta(&root, "backlog split")?;
    let discussion_budgets = resolve_ticket_discussion_budgets(&planning_meta);
    ensure_planning_layout(&root, false)?;
    ensure_backlog_templates(&root, false)?;
    let LinearCommandContext { service, .. } = load_linear_command_context(&args.client, None)?;
    let source = service.load_issue(&args.issue).await?;

    let can_launch_tui = io::stdin().is_terminal() && io::stdout().is_terminal();
    if args.no_interactive || (!args.render_once && !can_launch_tui) {
        let mut continuation = None;
        let proposal = generate_split_proposal(
            &root,
            &source,
            discussion_budgets,
            None,
            args,
            &mut continuation,
        )?;
        return Ok(SplitReport::Proposed(Box::new(ProposedSplitReport {
            source,
            proposal,
        })));
    }

    if args.render_once {
        return match run_render_once_session(&root, source.clone(), discussion_budgets, args)? {
            RenderOnceSplitResult::Snapshot(snapshot) => Ok(SplitReport::Rendered(snapshot)),
            RenderOnceSplitResult::Confirmed {
                proposal,
                selected_indices,
                selected_dependency_indices,
            } => {
                let remembered_selection = load_remembered_backlog_selection(&root)?;
                let apply_context = SplitApplyContext {
                    root: &root,
                    app_config: &app_config,
                    planning_meta: &planning_meta,
                    remembered_selection: &remembered_selection,
                    service: &service,
                    source: &source,
                    args,
                };
                apply_split(
                    &apply_context,
                    &proposal,
                    &selected_indices,
                    &selected_dependency_indices,
                )
                .await
            }
        };
    }

    match run_interactive_split_session(&root, source.clone(), discussion_budgets, args)? {
        InteractiveSplitExit::Cancelled => Ok(SplitReport::Cancelled),
        InteractiveSplitExit::Confirmed {
            proposal,
            selected_indices,
            selected_dependency_indices,
        } => {
            let remembered_selection = load_remembered_backlog_selection(&root)?;
            let apply_context = SplitApplyContext {
                root: &root,
                app_config: &app_config,
                planning_meta: &planning_meta,
                remembered_selection: &remembered_selection,
                service: &service,
                source: &source,
                args,
            };
            apply_split(
                &apply_context,
                &proposal,
                &selected_indices,
                &selected_dependency_indices,
            )
            .await
        }
    }
}

fn generate_split_proposal(
    root: &Path,
    source: &IssueSummary,
    discussion_budgets: TicketDiscussionBudgets,
    addendum: Option<&str>,
    args: &SplitArgs,
    continuation: &mut Option<AgentContinuation>,
) -> Result<SplitProposal> {
    let prepared_context = prepare_issue_context(source, discussion_budgets);
    let prompt = render_split_prompt(root, &prepared_context, addendum)?;
    let output = run_agent_capture_with_continuation(
        &RunAgentArgs {
            root: Some(root.to_path_buf()),
            route_key: Some(AGENT_ROUTE_BACKLOG_SPLIT.to_string()),
            agent: args.agent.clone(),
            prompt,
            instructions: None,
            model: args.model.clone(),
            reasoning: args.reasoning.clone(),
            transport: None,
            attachments: Vec::new(),
        },
        continuation,
    )
    .with_context(|| {
        format!(
            "{} backlog split requires a configured local agent to generate split proposals",
            branding::COMMAND_NAME
        )
    })?;
    let proposal: SplitProposal = parse_agent_json(&output.stdout, "split proposal generation")?;
    validate_split_proposal(proposal)
}

fn render_split_prompt(
    root: &Path,
    prepared_context: &PreparedIssueContext,
    addendum: Option<&str>,
) -> Result<String> {
    let workflow_contract = load_compact_workflow_contract(root)?;
    let repository_context = load_context_bundle(root)?;
    let repository_snapshot = render_repository_snapshot(root)?;
    let source = &prepared_context.issue;
    let source_description = source
        .description
        .as_deref()
        .unwrap_or("_No Linear description was provided._");
    let parent_context = source
        .parent
        .as_ref()
        .and_then(|issue| issue.description.as_deref())
        .unwrap_or("_No parent description was provided._");
    let discussion = if prepared_context.prompt_discussion.trim().is_empty() {
        "_No Linear comments were provided._".to_string()
    } else {
        prepared_context.prompt_discussion.clone()
    };
    let image_summary = render_ticket_image_summary(&prepared_context.images);
    let addendum_block = addendum
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("Operator addendum for refinement:\n{value}\n\n"))
        .unwrap_or_default();

    Ok(format!(
        "You are generating an inverse-planning split proposal for an existing Linear issue.\n\n\
Injected workflow contract:\n{workflow_contract}\n\n\
Source issue:\n\
- Identifier: `{}`\n\
- Title: {}\n\
- State: {}\n\
- URL: {}\n\
- Description:\n{}\n\n\
Source parent context:\n{}\n\n\
Ticket discussion context:\n{}\n\n\
Localized ticket images:\n{}\n\n\
{}\n\
Repository planning context:\n{}\n\n\
Repository directory snapshot:\n{}\n\n\
Instructions:\n\
1. Propose multiple independently understandable child issues for this repository only.\n\
2. Rewrite the source issue into an umbrella parent that summarizes the split without duplicating every child detail.\n\
3. Suggest dependency links only when one proposed child should block another.\n\
4. Keep child issue descriptions and acceptance criteria concrete enough to stand alone.\n\
5. Default scope to the full repository root unless the issue explicitly narrows it.\n\
6. Return JSON only with this exact shape:\n\
{{\n\
  \"summary\": \"One-paragraph rationale for the split\",\n\
  \"child_issues\": [\n\
    {{\n\
      \"proposal_id\": \"child-1\",\n\
      \"title\": \"...\",\n\
      \"description\": \"...\",\n\
      \"acceptance_criteria\": [\"...\"],\n\
      \"priority\": 2\n\
    }}\n\
  ],\n\
  \"parent_rewrite\": {{\n\
    \"title\": \"...\",\n\
    \"description\": \"...\",\n\
    \"acceptance_criteria\": [\"...\"]\n\
  }},\n\
  \"dependency_suggestions\": [\n\
    {{\n\
      \"blocking\": \"child-1\",\n\
      \"blocked\": \"child-2\",\n\
      \"rationale\": \"...\"\n\
    }}\n\
  ]\n\
}}",
        source.identifier,
        source.title,
        source
            .state
            .as_ref()
            .map(|state| state.name.as_str())
            .unwrap_or("Unknown"),
        source.url,
        source_description,
        parent_context,
        discussion,
        image_summary,
        addendum_block,
        repository_context,
        repository_snapshot,
    ))
}

fn validate_split_proposal(mut proposal: SplitProposal) -> Result<SplitProposal> {
    proposal.summary = proposal.summary.trim().to_string();
    proposal.parent_rewrite.title = proposal.parent_rewrite.title.trim().to_string();
    proposal.parent_rewrite.description = proposal.parent_rewrite.description.trim().to_string();
    proposal.parent_rewrite.acceptance_criteria = proposal
        .parent_rewrite
        .acceptance_criteria
        .into_iter()
        .map(|criterion| criterion.trim().to_string())
        .filter(|criterion| !criterion.is_empty())
        .collect();

    if proposal.parent_rewrite.title.is_empty() || proposal.parent_rewrite.description.is_empty() {
        bail!("split proposal parent rewrite must include a non-empty title and description");
    }

    if proposal.child_issues.is_empty() {
        bail!("split proposal must include at least one child issue");
    }

    let mut seen_ids = BTreeSet::new();
    for (index, child) in proposal.child_issues.iter_mut().enumerate() {
        if child.proposal_id.trim().is_empty() {
            child.proposal_id = format!("child-{}", index + 1);
        } else {
            child.proposal_id = child.proposal_id.trim().to_string();
        }
        child.title = child.title.trim().to_string();
        child.description = child.description.trim().to_string();
        child.acceptance_criteria = child
            .acceptance_criteria
            .iter()
            .map(|criterion| criterion.trim().to_string())
            .filter(|criterion| !criterion.is_empty())
            .collect();

        if child.title.is_empty() || child.description.is_empty() {
            bail!(
                "split proposal child `{}` must include a non-empty title and description",
                child.proposal_id
            );
        }
        if !seen_ids.insert(child.proposal_id.clone()) {
            bail!(
                "split proposal returned duplicate child proposal id `{}`",
                child.proposal_id
            );
        }
    }

    for dependency in &proposal.dependency_suggestions {
        if dependency.blocking == dependency.blocked {
            bail!(
                "split proposal contains a self-referential dependency for `{}`",
                dependency.blocking
            );
        }
        if !seen_ids.contains(&dependency.blocking) || !seen_ids.contains(&dependency.blocked) {
            bail!(
                "split proposal dependency `{}` -> `{}` references an unknown child proposal",
                dependency.blocking,
                dependency.blocked
            );
        }
    }

    Ok(proposal)
}

fn run_interactive_split_session(
    root: &Path,
    source: IssueSummary,
    discussion_budgets: TicketDiscussionBudgets,
    args: &SplitArgs,
) -> Result<InteractiveSplitExit> {
    let mut app = SplitApp::loading(source);
    start_split_initial_generation(&mut app, root, discussion_budgets, args.clone());

    let mut stdout = io::stdout();
    enable_raw_mode().context("failed to enable raw mode for split review")?;
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture
    )
    .context("failed to enter the split review screen")?;
    let _cleanup = TerminalCleanup;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal =
        Terminal::new(backend).context("failed to initialize the split review terminal")?;

    loop {
        process_pending_split_job(&mut app)?;
        terminal.draw(|frame| render_split_session(frame, &app))?;

        if event::poll(Duration::from_millis(250))
            .context("failed while polling split review input")?
        {
            match event::read().context("failed to read split review input")? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if top_level_cancel(key) || key.code == KeyCode::Esc {
                        return Ok(InteractiveSplitExit::Cancelled);
                    }

                    if is_mouse_toggle_key(key) {
                        app.copy.toggle_mouse_capture(terminal.backend_mut())?;
                        continue;
                    }

                    if app.loading.is_some() {
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

                    if is_copy_key(key) {
                        app.copy.copy_payload(app.copy_payload());
                        continue;
                    }

                    if let Some(exit) =
                        handle_split_key(&mut app, root, discussion_budgets, args, key)?
                    {
                        return Ok(exit);
                    }
                }
                Event::Paste(text) => {
                    if app.loading.is_none() && app.stage == SplitStage::Addendum {
                        let _ = app.addendum.paste(&text);
                        clear_split_error(&mut app);
                    }
                }
                Event::Mouse(mouse)
                    if matches!(
                        mouse.kind,
                        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                    ) =>
                {
                    if app.loading.is_some() {
                        continue;
                    }

                    let terminal_area = terminal.size()?.into();
                    if app.copy.export_active() {
                        let _ = app
                            .copy
                            .handle_export_mouse(mouse, copy_overlay_viewport(terminal_area));
                        continue;
                    }

                    let viewport = preview_viewport(terminal_area, app.stage);
                    let _ = app.preview_scroll.apply_mouse_in_viewport(
                        mouse,
                        viewport,
                        app.preview_rows(viewport.width.max(1)),
                    );
                }
                _ => {}
            }
        } else {
            advance_loading_spinner(&mut app);
        }
    }
}

fn run_render_once_session(
    root: &Path,
    source: IssueSummary,
    discussion_budgets: TicketDiscussionBudgets,
    args: &SplitArgs,
) -> Result<RenderOnceSplitResult> {
    let backend = TestBackend::new(args.width, args.height);
    let mut terminal =
        Terminal::new(backend).context("failed to initialize split render-once backend")?;
    let mut app = SplitApp::loading(source);
    start_split_initial_generation(&mut app, root, discussion_budgets, args.clone());

    for event in &args.events {
        process_pending_split_job_blocking(&mut app)?;
        let key = match event {
            SplitReviewEventArg::Enter => Some(KeyEvent::new(
                KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            )),
            SplitReviewEventArg::Left => Some(KeyEvent::new(
                KeyCode::Left,
                crossterm::event::KeyModifiers::NONE,
            )),
            SplitReviewEventArg::Up => Some(KeyEvent::new(
                KeyCode::Up,
                crossterm::event::KeyModifiers::NONE,
            )),
            SplitReviewEventArg::Down => Some(KeyEvent::new(
                KeyCode::Down,
                crossterm::event::KeyModifiers::NONE,
            )),
            SplitReviewEventArg::Space => Some(KeyEvent::new(
                KeyCode::Char(' '),
                crossterm::event::KeyModifiers::NONE,
            )),
            SplitReviewEventArg::Paste(text) => {
                if app.loading.is_none() && app.stage == SplitStage::Addendum {
                    let _ = app.addendum.paste(text);
                    clear_split_error(&mut app);
                }
                None
            }
        };

        if let Some(key) = key
            && let Some(exit) = handle_split_key(&mut app, root, discussion_budgets, args, key)?
        {
            return match exit {
                InteractiveSplitExit::Cancelled => Ok(RenderOnceSplitResult::Snapshot(
                    "Backlog split cancelled.".to_string(),
                )),
                InteractiveSplitExit::Confirmed {
                    proposal,
                    selected_indices,
                    selected_dependency_indices,
                } => Ok(RenderOnceSplitResult::Confirmed {
                    proposal,
                    selected_indices,
                    selected_dependency_indices,
                }),
            };
        }
    }

    process_pending_split_job_blocking(&mut app)?;
    terminal
        .draw(|frame| render_split_session(frame, &app))
        .context("failed to render split snapshot")?;
    Ok(RenderOnceSplitResult::Snapshot(snapshot(
        terminal.backend(),
    )))
}

fn handle_split_key(
    app: &mut SplitApp,
    root: &Path,
    discussion_budgets: TicketDiscussionBudgets,
    args: &SplitArgs,
    key: KeyEvent,
) -> Result<Option<InteractiveSplitExit>> {
    if app.loading.is_some() {
        return Ok(None);
    }

    let stage = app.stage;
    match key.code {
        KeyCode::Left | KeyCode::BackTab => {
            app.stage = match stage {
                SplitStage::Source => SplitStage::Source,
                SplitStage::Children => SplitStage::Source,
                SplitStage::Dependencies => SplitStage::Children,
                SplitStage::Addendum => SplitStage::Dependencies,
                SplitStage::Confirm => SplitStage::Addendum,
            };
            app.preview_scroll.reset();
            clear_split_error_for_navigation(app);
            Ok(None)
        }
        KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End => {
            let viewport = preview_viewport(Rect::new(0, 0, 120, 32), app.stage);
            let _ = app.preview_scroll.apply_key_in_viewport(
                key,
                viewport,
                app.preview_rows(viewport.width.max(1)),
            );
            clear_split_error_for_navigation(app);
            Ok(None)
        }
        KeyCode::Up if app.stage == SplitStage::Children => {
            let _ = app.selected_children.handle_key(key);
            app.sync_dependency_selection();
            app.preview_scroll.reset();
            clear_split_error_for_navigation(app);
            Ok(None)
        }
        KeyCode::Down if app.stage == SplitStage::Children => {
            let _ = app.selected_children.handle_key(key);
            app.sync_dependency_selection();
            app.preview_scroll.reset();
            clear_split_error_for_navigation(app);
            Ok(None)
        }
        KeyCode::Char(' ') if app.stage == SplitStage::Children => {
            let _ = app.selected_children.handle_key(key);
            app.sync_dependency_selection();
            clear_split_error(app);
            Ok(None)
        }
        KeyCode::Up if app.stage == SplitStage::Dependencies => {
            let _ = app.selected_dependencies.handle_key(key);
            app.preview_scroll.reset();
            clear_split_error_for_navigation(app);
            Ok(None)
        }
        KeyCode::Down if app.stage == SplitStage::Dependencies => {
            let _ = app.selected_dependencies.handle_key(key);
            app.preview_scroll.reset();
            clear_split_error_for_navigation(app);
            Ok(None)
        }
        KeyCode::Char(' ') if app.stage == SplitStage::Dependencies => {
            let _ = app.selected_dependencies.handle_key(key);
            clear_split_error(app);
            Ok(None)
        }
        KeyCode::Enter => match app.stage {
            SplitStage::Source => {
                app.stage = SplitStage::Children;
                app.preview_scroll.reset();
                clear_split_error_for_navigation(app);
                Ok(None)
            }
            SplitStage::Children => {
                if app.selected_children.selected_indices().is_empty() {
                    app.error = Some(
                        "Select at least one child issue before continuing to dependencies."
                            .to_string(),
                    );
                    app.sticky_error = false;
                    return Ok(None);
                }
                app.stage = SplitStage::Dependencies;
                app.preview_scroll.reset();
                clear_split_error(app);
                Ok(None)
            }
            SplitStage::Dependencies => {
                app.stage = SplitStage::Addendum;
                app.preview_scroll.reset();
                clear_split_error_for_navigation(app);
                Ok(None)
            }
            SplitStage::Addendum => {
                let addendum = app.addendum.value().trim().to_string();
                if addendum.is_empty() {
                    app.stage = SplitStage::Confirm;
                    app.preview_scroll.reset();
                    clear_split_error_for_navigation(app);
                    return Ok(None);
                }

                start_split_refinement(app, root, discussion_budgets, args.clone(), addendum);
                Ok(None)
            }
            SplitStage::Confirm => Ok(Some(InteractiveSplitExit::Confirmed {
                proposal: app.proposal.clone(),
                selected_indices: app.selected_children.selected_indices(),
                selected_dependency_indices: app.selected_dependency_indices(),
            })),
        },
        _ if app.stage == SplitStage::Addendum && app.addendum.handle_key(key) => {
            clear_split_error(app);
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn clear_split_error(app: &mut SplitApp) {
    app.error = None;
    app.sticky_error = false;
}

fn clear_split_error_for_navigation(app: &mut SplitApp) {
    if !app.sticky_error {
        app.error = None;
    }
}

fn set_split_sticky_error(app: &mut SplitApp, message: String) {
    app.error = Some(message);
    app.sticky_error = true;
}

fn start_split_refinement(
    app: &mut SplitApp,
    root: &Path,
    discussion_budgets: TicketDiscussionBudgets,
    args: SplitArgs,
    addendum: String,
) {
    app.loading = Some(LoadingApp {
        message: "Refining split proposal".to_string(),
        detail: "Rebuilding the child issues, parent rewrite, and dependency suggestions with your guidance.".to_string(),
        spinner_index: 0,
    });
    app.pending = Some(PendingSplitJob {
        receiver: spawn_split_refinement_job(
            root.to_path_buf(),
            app.source.clone(),
            discussion_budgets,
            args,
            addendum,
            app.continuation.clone(),
        ),
        mode: PendingSplitMode::Refinement,
    });
}

fn start_split_initial_generation(
    app: &mut SplitApp,
    root: &Path,
    discussion_budgets: TicketDiscussionBudgets,
    args: SplitArgs,
) {
    app.loading = Some(LoadingApp {
        message: "Generating split proposal".to_string(),
        detail:
            "Reviewing the issue context and drafting child issues before the review flow opens."
                .to_string(),
        spinner_index: 0,
    });
    app.pending = Some(PendingSplitJob {
        receiver: spawn_split_refinement_job(
            root.to_path_buf(),
            app.source.clone(),
            discussion_budgets,
            args,
            String::new(),
            None,
        ),
        mode: PendingSplitMode::InitialGeneration,
    });
}

fn spawn_split_refinement_job(
    root: PathBuf,
    source: IssueSummary,
    discussion_budgets: TicketDiscussionBudgets,
    args: SplitArgs,
    addendum: String,
    continuation: Option<AgentContinuation>,
) -> Receiver<SplitWorkerReport> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut continuation = continuation;
        let outcome = generate_split_proposal(
            &root,
            &source,
            discussion_budgets,
            Some(&addendum),
            &args,
            &mut continuation,
        );
        let _ = sender.send(SplitWorkerReport {
            continuation,
            outcome,
        });
    });
    receiver
}

fn process_pending_split_job(app: &mut SplitApp) -> Result<()> {
    let Some(pending) = app.pending.as_ref() else {
        return Ok(());
    };

    match pending.receiver.try_recv() {
        Ok(report) => finish_pending_split_job(app, report),
        Err(TryRecvError::Empty) => Ok(()),
        Err(TryRecvError::Disconnected) => match pending.mode {
            PendingSplitMode::InitialGeneration => {
                bail!("split generation worker exited before returning a result")
            }
            PendingSplitMode::Refinement => restore_split_after_error(
                app,
                "split refinement worker exited before returning a result".to_string(),
            ),
        },
    }
}

fn process_pending_split_job_blocking(app: &mut SplitApp) -> Result<()> {
    let Some(pending) = app.pending.as_ref() else {
        return Ok(());
    };

    let report = pending
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|error| {
            anyhow!("split refinement worker did not finish before render-once timeout: {error}")
        })?;
    finish_pending_split_job(app, report)
}

fn finish_pending_split_job(app: &mut SplitApp, report: SplitWorkerReport) -> Result<()> {
    let pending = app
        .pending
        .take()
        .ok_or_else(|| anyhow!("split refinement job disappeared unexpectedly"))?;
    app.loading = None;
    app.continuation = report.continuation;

    match report.outcome {
        Ok(proposal) => {
            app.proposal = proposal;
            app.selected_children = SplitApp::selection_for(&app.proposal);
            app.selected_dependencies = SplitApp::dependency_selection_for(
                &app.proposal,
                &app.selected_child_indices(),
                None,
            );
            app.addendum = InputFieldState::default();
            app.stage = SplitStage::Source;
            app.preview_scroll.reset();
            clear_split_error(app);
            Ok(())
        }
        Err(error) => match pending.mode {
            PendingSplitMode::InitialGeneration => Err(error),
            PendingSplitMode::Refinement => restore_split_after_error(app, error.to_string()),
        },
    }
}

fn restore_split_after_error(app: &mut SplitApp, message: String) -> Result<()> {
    let _pending = app.pending.take();
    app.loading = None;
    app.stage = SplitStage::Addendum;
    set_split_sticky_error(app, message);
    Ok(())
}

fn advance_loading_spinner(app: &mut SplitApp) {
    if let Some(loading) = &mut app.loading {
        loading.spinner_index = (loading.spinner_index + 1) % SPINNER_FRAMES.len();
    }
}

impl SplitApp {
    fn loading(source: IssueSummary) -> Self {
        Self {
            source,
            selected_children: MultiSelectFieldState::new(Vec::new(), []),
            selected_dependencies: MultiSelectFieldState::new(Vec::new(), []),
            proposal: Self::empty_proposal(),
            stage: SplitStage::Source,
            preview_scroll: ScrollState::default(),
            addendum: InputFieldState::default(),
            error: None,
            sticky_error: false,
            copy: CopyUiState::default(),
            continuation: None,
            loading: None,
            pending: None,
        }
    }

    #[cfg(test)]
    fn new(
        source: IssueSummary,
        proposal: SplitProposal,
        continuation: Option<AgentContinuation>,
    ) -> Self {
        let selected_children = Self::selection_for(&proposal);
        let selected_child_indices = selected_children.selected_indices();
        Self {
            source,
            selected_children,
            selected_dependencies: Self::dependency_selection_for(
                &proposal,
                &selected_child_indices,
                None,
            ),
            proposal,
            stage: SplitStage::Source,
            preview_scroll: ScrollState::default(),
            addendum: InputFieldState::default(),
            error: None,
            sticky_error: false,
            copy: CopyUiState::default(),
            continuation,
            loading: None,
            pending: None,
        }
    }

    fn empty_proposal() -> SplitProposal {
        SplitProposal {
            summary: String::new(),
            child_issues: Vec::new(),
            parent_rewrite: SplitParentDraft {
                title: String::new(),
                description: String::new(),
                acceptance_criteria: Vec::new(),
            },
            dependency_suggestions: Vec::new(),
        }
    }

    fn selection_for(proposal: &SplitProposal) -> MultiSelectFieldState {
        MultiSelectFieldState::new(
            proposal
                .child_issues
                .iter()
                .map(|child| format!("{}  {}", child.proposal_id, child.title))
                .collect::<Vec<_>>(),
            0..proposal.child_issues.len(),
        )
    }

    fn copy_payload(&self) -> CopyPayload {
        match self.stage {
            SplitStage::Source => CopyPayload::markdown(
                "backlog split source issue",
                render_issue_markdown(
                    &self.source.title,
                    self.source.description.as_deref().unwrap_or_default(),
                    &[],
                ),
            ),
            SplitStage::Children => self
                .selected_child()
                .map(|child| {
                    CopyPayload::markdown(
                        format!("backlog split child {}", child.proposal_id),
                        render_issue_markdown(
                            &child.title,
                            &child.description,
                            &child.acceptance_criteria,
                        ),
                    )
                })
                .unwrap_or_else(|| CopyPayload::new("backlog split child", "No child selected.")),
            SplitStage::Dependencies => CopyPayload::new(
                "backlog split dependencies",
                self.dependency_preview_markdown(),
            ),
            SplitStage::Addendum => self.addendum.copy_payload("backlog split addendum"),
            SplitStage::Confirm => CopyPayload::markdown(
                "backlog split parent rewrite",
                render_issue_markdown(
                    &self.proposal.parent_rewrite.title,
                    &self.proposal.parent_rewrite.description,
                    &self.proposal.parent_rewrite.acceptance_criteria,
                ),
            ),
        }
    }

    fn selected_child(&self) -> Option<&SplitChildDraft> {
        self.proposal
            .child_issues
            .get(self.selected_children.cursor())
            .or_else(|| self.proposal.child_issues.first())
    }

    fn selected_child_indices(&self) -> Vec<usize> {
        self.selected_children.selected_indices()
    }

    fn dependency_key(dependency: &SplitDependencySuggestion) -> DependencyKey {
        (dependency.blocking.clone(), dependency.blocked.clone())
    }

    fn visible_dependency_indices_for(
        proposal: &SplitProposal,
        selected_child_indices: &[usize],
    ) -> Vec<usize> {
        let selected_ids = selected_child_indices
            .iter()
            .filter_map(|index| proposal.child_issues.get(*index))
            .map(|child| child.proposal_id.clone())
            .collect::<BTreeSet<_>>();

        proposal
            .dependency_suggestions
            .iter()
            .enumerate()
            .filter(|dependency| {
                selected_ids.contains(&dependency.1.blocking)
                    && selected_ids.contains(&dependency.1.blocked)
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn dependency_selection_for(
        proposal: &SplitProposal,
        selected_child_indices: &[usize],
        previous_selection: Option<&DependencySelectionState>,
    ) -> MultiSelectFieldState {
        let visible_indices =
            Self::visible_dependency_indices_for(proposal, selected_child_indices);
        let options = visible_indices
            .iter()
            .filter_map(|index| proposal.dependency_suggestions.get(*index))
            .map(|dependency| format!("{} blocks {}", dependency.blocking, dependency.blocked))
            .collect::<Vec<_>>();
        let selected = visible_indices
            .iter()
            .enumerate()
            .filter_map(|(visible_index, dependency_index)| {
                let dependency = proposal.dependency_suggestions.get(*dependency_index)?;
                let key = Self::dependency_key(dependency);
                let should_select = match previous_selection {
                    Some((previous_visible, previous_selected)) => {
                        !previous_visible.contains(&key) || previous_selected.contains(&key)
                    }
                    None => true,
                };
                should_select.then_some(visible_index)
            })
            .collect::<Vec<_>>();
        MultiSelectFieldState::new(options, selected)
    }

    fn visible_dependency_indices(&self) -> Vec<usize> {
        Self::visible_dependency_indices_for(&self.proposal, &self.selected_child_indices())
    }

    fn visible_dependency_suggestions(&self) -> Vec<&SplitDependencySuggestion> {
        self.visible_dependency_indices()
            .into_iter()
            .filter_map(|index| self.proposal.dependency_suggestions.get(index))
            .collect()
    }

    fn visible_dependency_keys(&self) -> BTreeSet<DependencyKey> {
        self.visible_dependency_suggestions()
            .into_iter()
            .map(Self::dependency_key)
            .collect()
    }

    fn selected_dependency_indices(&self) -> Vec<usize> {
        let visible = self.visible_dependency_indices();
        self.selected_dependencies
            .selected_indices()
            .into_iter()
            .filter_map(|index| visible.get(index).copied())
            .collect()
    }

    fn selected_dependency_keys(&self) -> BTreeSet<DependencyKey> {
        self.selected_dependency_indices()
            .into_iter()
            .filter_map(|index| self.proposal.dependency_suggestions.get(index))
            .map(Self::dependency_key)
            .collect()
    }

    fn selected_dependency_suggestions(&self) -> Vec<&SplitDependencySuggestion> {
        self.selected_dependency_indices()
            .into_iter()
            .filter_map(|index| self.proposal.dependency_suggestions.get(index))
            .collect()
    }

    fn selected_dependency(&self) -> Option<&SplitDependencySuggestion> {
        let visible = self.visible_dependency_indices();
        visible
            .get(self.selected_dependencies.cursor())
            .and_then(|index| self.proposal.dependency_suggestions.get(*index))
            .or_else(|| self.visible_dependency_suggestions().into_iter().next())
    }

    fn sync_dependency_selection(&mut self) {
        let previous_visible = self.visible_dependency_keys();
        let previous_selected = self.selected_dependency_keys();
        self.selected_dependencies = Self::dependency_selection_for(
            &self.proposal,
            &self.selected_child_indices(),
            Some(&(previous_visible, previous_selected)),
        );
    }

    fn dependency_preview_markdown(&self) -> String {
        let mut lines = vec![
            format!(
                "Selected child issues: {}",
                self.selected_child_indices().len()
            ),
            format!(
                "Resolvable suggestions in scope: {}",
                self.visible_dependency_indices().len()
            ),
            format!(
                "Dependency links selected for apply: {}",
                self.selected_dependency_indices().len()
            ),
            String::new(),
            "Focused dependency".to_string(),
        ];
        if let Some(dependency) = self.selected_dependency() {
            if dependency.rationale.trim().is_empty() {
                lines.push(format!(
                    "- `{}` blocks `{}`",
                    dependency.blocking, dependency.blocked
                ));
            } else {
                lines.push(format!(
                    "- `{}` blocks `{}`: {}",
                    dependency.blocking, dependency.blocked, dependency.rationale
                ));
            }
        } else {
            lines.push(
                "- _No resolvable dependency suggestions remain for the current selection._"
                    .to_string(),
            );
        }
        lines.push(String::new());
        lines.push("Selected dependency links".to_string());
        let selected_dependencies = self.selected_dependency_suggestions();
        if selected_dependencies.is_empty() {
            lines.push("- _No dependency links will be created from this proposal._".to_string());
        } else {
            lines.extend(selected_dependencies.into_iter().map(|dependency| {
                if dependency.rationale.trim().is_empty() {
                    format!(
                        "- `{}` blocks `{}`",
                        dependency.blocking, dependency.blocked
                    )
                } else {
                    format!(
                        "- `{}` blocks `{}`: {}",
                        dependency.blocking, dependency.blocked, dependency.rationale
                    )
                }
            }));
        }
        lines.push(String::new());
        lines.push("Parent rewrite".to_string());
        lines.push(format!("- {}", self.proposal.parent_rewrite.title));
        lines.join("\n")
    }

    fn preview_rows(&self, width: u16) -> usize {
        let contents = match self.stage {
            SplitStage::Source => plain_text(&render_markdown(
                &render_issue_markdown(
                    &self.source.title,
                    self.source.description.as_deref().unwrap_or_default(),
                    &[],
                ),
                Style::default(),
                &[],
            )),
            SplitStage::Children => self
                .selected_child()
                .map(|child| {
                    plain_text(&render_markdown(
                        &render_issue_markdown(
                            &child.title,
                            &child.description,
                            &child.acceptance_criteria,
                        ),
                        Style::default(),
                        &[],
                    ))
                })
                .unwrap_or_default(),
            SplitStage::Dependencies => self.dependency_preview_markdown(),
            SplitStage::Addendum => self.addendum.value().to_string(),
            SplitStage::Confirm => plain_text(&render_markdown(
                &render_issue_markdown(
                    &self.proposal.parent_rewrite.title,
                    &self.proposal.parent_rewrite.description,
                    &self.proposal.parent_rewrite.acceptance_criteria,
                ),
                Style::default(),
                &[],
            )),
        };
        wrapped_rows(&contents, width.max(1))
    }
}

async fn apply_split<C>(
    context: &SplitApplyContext<'_, C>,
    proposal: &SplitProposal,
    selected_indices: &[usize],
    selected_dependency_indices: &[usize],
) -> Result<SplitReport>
where
    C: crate::linear::LinearClient,
{
    let root = context.root;
    let app_config = context.app_config;
    let planning_meta = context.planning_meta;
    let remembered_selection = context.remembered_selection;
    let service = context.service;
    let source = context.source;
    let args = context.args;
    let mut created_children = Vec::new();
    let mut backlog_paths = Vec::new();
    let mut child_lookup = BTreeMap::new();

    for &index in selected_indices {
        let draft = proposal
            .child_issues
            .get(index)
            .ok_or_else(|| anyhow!("selected split child index `{index}` is out of bounds"))?;
        let resolved_defaults = resolve_plan_ticket_defaults(
            app_config,
            planning_meta,
            remembered_selection,
            &PlanTicketResolutionInput {
                zero_prompt: false,
                explicit_team: Some(source.team.key.clone()),
                explicit_project: source.project.as_ref().map(|project| project.name.clone()),
                overrides: TicketOptionOverrides {
                    state: args.state.clone(),
                    priority: args.priority,
                    labels: args.labels.clone(),
                    assignee: args.assignee.clone(),
                },
                built_in_label: planning_meta.effective_plan_label(app_config),
                generated_priority: draft.priority,
            },
        );
        let initial_files = render_split_backlog_files(
            root,
            draft,
            TemplateContext {
                issue_title: Some(draft.title.clone()),
                ..TemplateContext::default()
            },
        )?;
        let initial_description = rendered_index_contents(&initial_files)?;
        let assignee_id = service
            .resolve_assignee_id(resolved_defaults.assignee.as_deref())
            .await?;
        let child = service
            .create_issue(IssueCreateSpec {
                team: resolved_defaults.team.clone(),
                title: draft.title.clone(),
                description: Some(initial_description),
                project: resolved_defaults.project.clone(),
                project_id: resolved_defaults.project_id.clone(),
                project_milestone_id: None,
                parent_id: Some(source.id.clone()),
                state: resolved_defaults.state.clone(),
                priority: resolved_defaults.priority,
                assignee_id,
                labels: resolved_defaults.labels.clone(),
            })
            .await
            .with_context(|| {
                format!(
                    "failed to create split child `{}` for parent `{}`",
                    draft.proposal_id, source.identifier
                )
            })?;
        if let Err(error) = save_remembered_backlog_selection(root, &child) {
            eprintln!("warning: failed to persist remembered backlog defaults: {error}");
        }
        created_children.push(child.clone());
        child_lookup.insert(draft.proposal_id.clone(), child.clone());

        let rendered_files = render_split_backlog_files(
            root,
            draft,
            TemplateContext {
                issue_identifier: Some(child.identifier.clone()),
                issue_title: Some(child.title.clone()),
                issue_url: Some(child.url.clone()),
                ..TemplateContext::default()
            },
        )
        .with_context(|| {
            format!(
                "created split children [{}] but failed to render the local backlog packet for `{}`",
                created_child_identifiers(&created_children),
                child.identifier
            )
        })?;
        let issue_dir = write_rendered_backlog_item(root, &child.identifier, &rendered_files)
            .with_context(|| {
                format!(
                    "created split children [{}] but failed to write the local backlog packet for `{}`",
                    created_child_identifiers(&created_children),
                    child.identifier
                )
            })?;
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
                parent_id: Some(source.id.clone()),
                parent_identifier: Some(source.identifier.clone()),
                local_hash: None,
                remote_hash: None,
                last_sync_at: None,
                last_pulled_comment_ids: Vec::new(),
                managed_files: Vec::<ManagedFileRecord>::new(),
            },
        )
        .with_context(|| {
            format!(
                "created split children [{}] but failed to write backlog metadata for `{}`",
                created_child_identifiers(&created_children),
                child.identifier
            )
        })?;
        backlog_paths.push(display_path(&issue_dir, root));
    }

    let rewritten_parent = match service
        .edit_issue(IssueEditSpec {
            identifier: source.identifier.clone(),
            title: Some(proposal.parent_rewrite.title.clone()),
            description: Some(render_issue_markdown(
                &proposal.parent_rewrite.title,
                &proposal.parent_rewrite.description,
                &proposal.parent_rewrite.acceptance_criteria,
            )),
            project: None,
            state: None,
            priority: None,
            estimate: None,
            labels: None,
            parent_identifier: None,
        })
        .await
    {
        Ok(parent) => parent,
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "created split children [{}] but failed to rewrite parent `{}`",
                    created_child_identifiers(&created_children),
                    source.identifier
                )
            });
        }
    };

    let mut dependency_links_created = 0usize;
    let mut dependency_link_notes = Vec::new();
    let mut seen_dependencies = BTreeSet::new();
    for &dependency_index in selected_dependency_indices {
        let dependency = proposal
            .dependency_suggestions
            .get(dependency_index)
            .ok_or_else(|| {
                anyhow!("selected split dependency index `{dependency_index}` is out of bounds")
            })?;
        let Some(blocking) = child_lookup.get(&dependency.blocking) else {
            dependency_link_notes.push(format!(
                "skipped `{}` -> `{}` because the blocking child was not selected",
                dependency.blocking, dependency.blocked
            ));
            continue;
        };
        let Some(blocked) = child_lookup.get(&dependency.blocked) else {
            dependency_link_notes.push(format!(
                "skipped `{}` -> `{}` because the blocked child was not selected",
                dependency.blocking, dependency.blocked
            ));
            continue;
        };
        if !seen_dependencies.insert((blocking.id.clone(), blocked.id.clone())) {
            continue;
        }
        if let Err(error) = service
            .create_issue_relation(&blocking.id, &blocked.id, "blocks")
            .await
        {
            dependency_link_notes.push(format!(
                "failed to link {} blocking {}: {error}",
                blocking.identifier, blocked.identifier
            ));
            continue;
        }
        dependency_links_created += 1;
    }

    Ok(SplitReport::Applied(Box::new(AppliedSplitReport {
        source: source.clone(),
        parent: rewritten_parent,
        children: created_children,
        backlog_paths,
        dependency_links_created,
        dependency_link_notes,
    })))
}

fn created_child_identifiers(children: &[IssueSummary]) -> String {
    children
        .iter()
        .map(|child| child.identifier.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_split_backlog_files(
    root: &Path,
    draft: &SplitChildDraft,
    context: TemplateContext,
) -> Result<Vec<RenderedTemplateFile>> {
    let mut rendered_files = render_template_files(root, &context)?;
    let index_file = rendered_files
        .iter_mut()
        .find(|file| file.relative_path == INDEX_FILE_NAME)
        .ok_or_else(|| anyhow!("the backlog template must contain `{INDEX_FILE_NAME}`"))?;
    index_file.contents =
        render_issue_markdown(&draft.title, &draft.description, &draft.acceptance_criteria);
    ensure_no_unresolved_placeholders(&rendered_files)?;
    Ok(rendered_files)
}

fn rendered_index_contents(rendered_files: &[RenderedTemplateFile]) -> Result<String> {
    rendered_files
        .iter()
        .find(|file| file.relative_path == INDEX_FILE_NAME)
        .map(|file| file.contents.clone())
        .ok_or_else(|| anyhow!("the backlog template must contain `{INDEX_FILE_NAME}`"))
}

fn render_issue_markdown(title: &str, description: &str, acceptance_criteria: &[String]) -> String {
    let mut lines = vec![format!("# {title}"), String::new(), description.to_string()];
    if !acceptance_criteria.is_empty() {
        lines.push(String::new());
        lines.push("## Acceptance Criteria".to_string());
        lines.push(String::new());
        lines.extend(
            acceptance_criteria
                .iter()
                .map(|criterion| format!("- {criterion}")),
        );
    }
    lines.join("\n")
}

fn render_split_session(frame: &mut Frame<'_>, app: &SplitApp) {
    if let Some(loading) = &app.loading {
        render_loading_frame(frame, loading);
        return;
    }

    let layout = base_layout(frame.area());
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
        .split(layout[0]);

    match app.stage {
        SplitStage::Source => render_source_stage(frame, app, &body),
        SplitStage::Children => render_children_stage(frame, app, &body),
        SplitStage::Dependencies => render_dependencies_stage(frame, app, &body),
        SplitStage::Addendum => render_addendum_stage(frame, app, &body),
        SplitStage::Confirm => render_confirm_stage(frame, app, &body),
    }

    render_footer(
        frame,
        layout[1],
        app.error.as_deref(),
        app.copy.status_text(),
        app.stage,
    );
    app.copy.render_export_overlay(frame, frame.area());
}

fn render_source_stage(frame: &mut Frame<'_>, app: &SplitApp, body: &[Rect]) {
    let summary = Paragraph::new(Text::from(vec![
        Line::from(format!("Source: {}", app.source.identifier)),
        Line::from(app.source.title.clone()),
        Line::from(""),
        Line::from(format!("Proposed children: {}", app.proposal.child_issues.len())),
        Line::from(format!(
            "Suggested dependencies: {}",
            app.proposal.dependency_suggestions.len()
        )),
        Line::from(""),
        Line::styled(
            "Review the source ticket first, then step through child selection, dependencies, addendum, and confirmation.",
            Style::default().add_modifier(Modifier::DIM),
        ),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Split Summary"))
    .wrap(Wrap { trim: false });
    frame.render_widget(summary, body[0]);

    let markdown = render_markdown(
        &render_issue_markdown(
            &app.source.title,
            app.source.description.as_deref().unwrap_or_default(),
            &[],
        ),
        Style::default(),
        &[],
    );
    let preview = scrollable_content_paragraph(markdown, "Source Issue", &app.preview_scroll)
        .wrap(Wrap { trim: false });
    frame.render_widget(preview, body[1]);
}

fn render_children_stage(frame: &mut Frame<'_>, app: &SplitApp, body: &[Rect]) {
    let mut state = ListState::default();
    state.select(Some(
        app.selected_children
            .cursor()
            .min(app.proposal.child_issues.len().saturating_sub(1)),
    ));
    let selected = app.selected_children.selected_indices();
    let items = app
        .proposal
        .child_issues
        .iter()
        .enumerate()
        .map(|(index, child)| {
            let marker = if selected.contains(&index) {
                "[x]"
            } else {
                "[ ]"
            };
            ListItem::new(format!("{marker} {}  {}", child.proposal_id, child.title))
        })
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!(
            "Child Tickets ({}/{})",
            selected.len(),
            app.proposal.child_issues.len()
        )))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, body[0], &mut state);

    let markdown = app
        .selected_child()
        .map(|child| {
            render_markdown(
                &render_issue_markdown(
                    &child.title,
                    &child.description,
                    &child.acceptance_criteria,
                ),
                Style::default(),
                &[],
            )
        })
        .unwrap_or_else(|| Text::from("No child issue selected."));
    let preview = scrollable_content_paragraph(markdown, "Child Preview", &app.preview_scroll)
        .wrap(Wrap { trim: false });
    frame.render_widget(preview, body[1]);
}

fn render_dependencies_stage(frame: &mut Frame<'_>, app: &SplitApp, body: &[Rect]) {
    let dependencies = app.visible_dependency_suggestions();
    let selected = app.selected_dependencies.selected_indices();
    let items = if dependencies.is_empty() {
        vec![ListItem::new(
            "No resolvable dependency suggestions remain.",
        )]
    } else {
        dependencies
            .iter()
            .enumerate()
            .map(|(index, dependency)| {
                let marker = if selected.contains(&index) {
                    "[x]"
                } else {
                    "[ ]"
                };
                ListItem::new(format!(
                    "{marker} {} blocks {}",
                    dependency.blocking, dependency.blocked
                ))
            })
            .collect::<Vec<_>>()
    };
    let mut state = ListState::default();
    if !dependencies.is_empty() {
        state.select(Some(
            app.selected_dependencies
                .cursor()
                .min(dependencies.len().saturating_sub(1)),
        ));
    }
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!(
            "Dependencies and Order ({}/{})",
            app.selected_dependency_indices().len(),
            dependencies.len()
        )))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, body[0], &mut state);

    let preview = scrollable_content_paragraph(
        render_markdown(&app.dependency_preview_markdown(), Style::default(), &[]),
        "Dependency Review",
        &app.preview_scroll,
    )
    .wrap(Wrap { trim: false });
    frame.render_widget(preview, body[1]);
}

fn render_addendum_stage(frame: &mut Frame<'_>, app: &SplitApp, body: &[Rect]) {
    let summary = Paragraph::new(Text::from(vec![
        Line::from(format!(
            "Selected children: {}",
            app.selected_child_indices().len()
        )),
        Line::from(format!(
            "Resolvable dependencies in scope: {}",
            app.visible_dependency_indices().len()
        )),
        Line::from(format!(
            "Dependency links selected: {}",
            app.selected_dependency_indices().len()
        )),
        Line::from(""),
        Line::styled(
            "Leave the addendum empty to keep the current proposal, or enter guidance to regenerate it before confirmation.",
            Style::default().add_modifier(Modifier::DIM),
        ),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Addendum"))
    .wrap(Wrap { trim: false });
    frame.render_widget(summary, body[0]);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(body[1]);
    let input_block = Block::default()
        .borders(Borders::ALL)
        .title("Operator Guidance");
    let input_inner = input_block.inner(right[0]);
    let rendered = app.addendum.render_with_width(
        "Type guidance to regenerate the split proposal, or press Enter on an empty field to continue...",
        true,
        input_inner.width,
    );
    frame.render_widget(rendered.paragraph(input_block), right[0]);
    rendered.set_cursor(frame, input_inner);

    let preview = Paragraph::new(Text::from(vec![
        Line::from("Current parent rewrite preview"),
        Line::from(""),
        Line::from(app.proposal.parent_rewrite.title.clone()),
        Line::from(""),
        Line::styled(
            app.proposal.parent_rewrite.description.clone(),
            Style::default().add_modifier(Modifier::DIM),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Current Proposal"),
    )
    .wrap(Wrap { trim: false });
    frame.render_widget(preview, right[1]);
}

fn render_confirm_stage(frame: &mut Frame<'_>, app: &SplitApp, body: &[Rect]) {
    let summary = Paragraph::new(Text::from(vec![
        Line::from(format!("Source: {}", app.source.identifier)),
        Line::from(format!(
            "Selected children: {}",
            app.selected_child_indices().len()
        )),
        Line::from(format!(
            "Dependency links to try: {}",
            app.selected_dependency_indices().len()
        )),
        Line::from(""),
        Line::styled(
            "Press Enter to apply the split: create the selected children, rewrite the parent, and link the dependency subset you kept in review.",
            Style::default().add_modifier(Modifier::DIM),
        ),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Confirm Apply"))
    .wrap(Wrap { trim: false });
    frame.render_widget(summary, body[0]);

    let preview = scrollable_content_paragraph(
        render_markdown(
            &render_issue_markdown(
                &app.proposal.parent_rewrite.title,
                &app.proposal.parent_rewrite.description,
                &app.proposal.parent_rewrite.acceptance_criteria,
            ),
            Style::default(),
            &[],
        ),
        "Parent Rewrite Preview",
        &app.preview_scroll,
    )
    .wrap(Wrap { trim: false });
    frame.render_widget(preview, body[1]);
}

fn render_footer(
    frame: &mut Frame<'_>,
    area: Rect,
    error: Option<&str>,
    status: Option<&str>,
    stage: SplitStage,
) {
    let help = match stage {
        SplitStage::Source => field_copy_help(
            "Enter moves to child review. PgUp/PgDn/Home/End or the mouse wheel scroll the source preview. Left/Esc cancels.",
        ),
        SplitStage::Children => pane_copy_help(
            "Up/Down moves through proposed children. Space toggles creation. Enter continues to dependency review. Left returns to the source review.",
        ),
        SplitStage::Dependencies => pane_copy_help(
            "Up/Down moves through the filtered dependency suggestions. Space toggles whether the focused link will be created. Enter continues to the addendum step. Left returns to child selection.",
        ),
        SplitStage::Addendum => field_copy_help(
            "Type guidance and press Enter to regenerate the proposal, or press Enter on an empty field to continue. Left returns to dependency review.",
        ),
        SplitStage::Confirm => pane_copy_help(
            "Enter applies the split. Left returns to the addendum step. Esc cancels the workflow.",
        ),
    };

    let mut lines = Vec::new();
    if let Some(message) = error.or(status) {
        lines.push(Line::styled(
            message.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::from(help));
    } else {
        lines.push(Line::from(help));
    }

    let footer = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::ALL).title("Controls"))
        .wrap(Wrap { trim: false });
    frame.render_widget(footer, area);
}

fn base_layout(area: Rect) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(5)])
        .split(area)
        .to_vec()
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

fn preview_viewport(area: Rect, stage: SplitStage) -> Rect {
    let layout = base_layout(area);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
        .split(layout[0]);
    let preview_area = if stage == SplitStage::Addendum {
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(body[1]);
        right[1]
    } else {
        body[1]
    };
    Rect::new(
        preview_area.x.saturating_add(1),
        preview_area.y.saturating_add(1),
        preview_area.width.saturating_sub(2).max(1),
        preview_area.height.saturating_sub(2).max(1),
    )
}

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
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(
            name.as_ref(),
            ".git" | "target" | "node_modules" | ".next" | "dist" | "build" | "coverage"
        ) {
            continue;
        }
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect `{}`", path.display()))?;
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let indent = "  ".repeat(depth);
        lines.push(if file_type.is_dir() {
            format!("{indent}- {}/", relative.display())
        } else {
            format!("{indent}- {}", relative.display())
        });
        *remaining = remaining.saturating_sub(1);
        if file_type.is_dir() {
            collect_directory_snapshot(root, &path, depth + 1, max_depth, remaining, lines)?;
        }
    }

    Ok(())
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
    eprintln!("warning: split JSON parse failed during {phase}; raw agent output:\n{trimmed}");
    bail!(
        "split agent returned invalid JSON during {phase}: {}",
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

#[cfg(test)]
mod tests {
    use super::{
        LoadingApp, SplitApp, SplitChildDraft, SplitDependencySuggestion, SplitParentDraft,
        SplitProposal, render_issue_markdown, render_loading_frame, render_split_session, snapshot,
        validate_split_proposal,
    };
    use crate::linear::{IssueSummary, ProjectRef, TeamRef, WorkflowState};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn issue() -> IssueSummary {
        IssueSummary {
            id: "issue-1".to_string(),
            identifier: "MET-35".to_string(),
            title: "Split the planning workflow".to_string(),
            description: Some("## Context\n\nSplit this issue into smaller tickets.".to_string()),
            url: "https://linear.app/issues/MET-35".to_string(),
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
                name: "Backlog".to_string(),
                kind: Some("backlog".to_string()),
            }),
            attachments: Vec::new(),
            parent: None,
            children: Vec::new(),
        }
    }

    fn proposal() -> SplitProposal {
        SplitProposal {
            summary: "Split summary".to_string(),
            child_issues: vec![
                SplitChildDraft {
                    proposal_id: "child-1".to_string(),
                    title: "Child one".to_string(),
                    description: "Build the first slice.".to_string(),
                    acceptance_criteria: vec!["It works".to_string()],
                    priority: Some(2),
                },
                SplitChildDraft {
                    proposal_id: "child-2".to_string(),
                    title: "Child two".to_string(),
                    description: "Build the second slice.".to_string(),
                    acceptance_criteria: vec!["It ships".to_string()],
                    priority: Some(3),
                },
            ],
            parent_rewrite: SplitParentDraft {
                title: "Umbrella parent".to_string(),
                description: "Tracks the split.".to_string(),
                acceptance_criteria: vec!["Children exist".to_string()],
            },
            dependency_suggestions: vec![SplitDependencySuggestion {
                blocking: "child-1".to_string(),
                blocked: "child-2".to_string(),
                rationale: "The backend lands first.".to_string(),
            }],
        }
    }

    #[test]
    fn validate_split_proposal_rejects_unknown_dependencies() {
        let error = validate_split_proposal(SplitProposal {
            dependency_suggestions: vec![SplitDependencySuggestion {
                blocking: "child-1".to_string(),
                blocked: "missing".to_string(),
                rationale: String::new(),
            }],
            ..proposal()
        })
        .expect_err("proposal should be rejected");
        assert!(error.to_string().contains("unknown child proposal"));
    }

    #[test]
    fn render_issue_markdown_includes_acceptance_criteria_section() {
        let markdown = render_issue_markdown("Title", "Description", &["One".to_string()]);
        assert!(markdown.contains("# Title"));
        assert!(markdown.contains("## Acceptance Criteria"));
        assert!(markdown.contains("- One"));
    }

    #[test]
    fn split_render_once_snapshot_surfaces_source_and_children() {
        let backend = TestBackend::new(120, 32);
        let mut terminal = Terminal::new(backend).expect("snapshot backend should initialize");
        let app = SplitApp::new(issue(), proposal(), None);
        terminal
            .draw(|frame| render_split_session(frame, &app))
            .expect("split session should render");
        let snapshot = snapshot(terminal.backend());
        assert!(snapshot.contains("Split Summary"));
        assert!(snapshot.contains("Source Issue"));
        assert!(snapshot.contains("Proposed children: 2"));
    }

    #[test]
    fn split_app_filters_dependencies_to_selected_children() {
        let mut app = SplitApp::new(issue(), proposal(), None);
        app.stage = super::SplitStage::Children;
        app.selected_children.toggle_current();
        app.selected_children.toggle_current();
        app.sync_dependency_selection();
        assert_eq!(app.selected_dependency_suggestions().len(), 1);
    }

    #[test]
    fn split_loading_snapshot_shows_agent_panel() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).expect("loading backend should initialize");
        let loading = LoadingApp {
            message: "Refining split proposal".to_string(),
            detail: "Rebuilding the draft with addendum guidance.".to_string(),
            spinner_index: 1,
        };
        terminal
            .draw(|frame| render_loading_frame(frame, &loading))
            .expect("loading frame should render");
        let snapshot = snapshot(terminal.backend());
        assert!(snapshot.contains("Agent Working [loading]"));
        assert!(snapshot.contains("Refining split proposal"));
    }
}

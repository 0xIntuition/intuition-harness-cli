use std::collections::BTreeSet;
use std::fs;
use std::io::{self, IsTerminal, Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseEventKind,
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
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use crate::agent_provider::builtin_provider_adapter;
use crate::agents::{
    AgentContinuation, AgentExecutionOptions, AgentTokenUsage, apply_invocation_environment,
    apply_noninteractive_agent_environment, command_args_for_invocation_with_options,
    validate_invocation_command_surface,
};
use crate::branding;
use crate::build::{BuildRunSummary, provider_display, render_summary, resolve_build_invocation};
use crate::cli::BuildArgs;
use crate::config::{AppConfig, PlanningMeta, PromptTransport};
use crate::fs::ensure_workspace_path_is_safe;
use crate::tui::copy::{CopyPayload, CopyUiState, copy_overlay_viewport, pane_copy_help};
use crate::tui::fields::InputFieldState;
use crate::tui::keybindings::{is_copy_key, is_mouse_toggle_key};
use crate::tui::scroll::{ScrollState, scrollable_paragraph_with_block, wrapped_rows};
use crate::tui::theme::{Tone, badge, emphasis_style, key_hints, muted_style, panel_title};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildFocus {
    Workspaces,
    Runs,
    Output,
    Status,
    Prompt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RunStatus {
    Running,
    Success,
    Failure(String),
    Interrupted,
}

impl RunStatus {
    fn label(&self) -> &str {
        match self {
            Self::Running => "running",
            Self::Success => "success",
            Self::Failure(_) => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    fn tone(&self) -> Tone {
        match self {
            Self::Running => Tone::Info,
            Self::Success => Tone::Success,
            Self::Failure(_) => Tone::Danger,
            Self::Interrupted => Tone::Muted,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct WorkspaceGitSnapshot {
    branch: String,
    changed_files: Vec<String>,
    modified_count: usize,
    deleted_count: usize,
    renamed_count: usize,
    untracked_count: usize,
    conflicted_count: usize,
    has_unpushed_commits: bool,
    is_detached: bool,
}

impl WorkspaceGitSnapshot {
    fn clean(&self) -> bool {
        self.changed_files.is_empty()
    }

    fn status_label(&self) -> String {
        let mut labels = Vec::new();
        if !self.clean() {
            labels.push(format!("{} changed", self.changed_files.len()));
        } else {
            labels.push("clean".to_string());
        }
        if self.untracked_count > 0 {
            labels.push(format!("{} untracked", self.untracked_count));
        }
        if self.has_unpushed_commits {
            labels.push("ahead".to_string());
        }
        if self.is_detached {
            labels.push("detached".to_string());
        }
        labels.join(" | ")
    }

    fn detail_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("Branch: {}", self.branch),
            format!("Status: {}", self.status_label()),
        ];
        if self.modified_count > 0 {
            lines.push(format!("Modified: {}", self.modified_count));
        }
        if self.deleted_count > 0 {
            lines.push(format!("Deleted: {}", self.deleted_count));
        }
        if self.renamed_count > 0 {
            lines.push(format!("Renamed: {}", self.renamed_count));
        }
        if self.untracked_count > 0 {
            lines.push(format!("Untracked: {}", self.untracked_count));
        }
        if self.conflicted_count > 0 {
            lines.push(format!("Conflicted: {}", self.conflicted_count));
        }
        lines
    }
}

#[derive(Debug, Clone)]
struct BuildRunEntry {
    number: u32,
    prompt: String,
    provider_label: String,
    status: RunStatus,
    output: String,
    usage: Option<AgentTokenUsage>,
    resumed_turns: u32,
    sync_summary: String,
    change_summary: String,
    publish_summary: String,
}

impl BuildRunEntry {
    fn summary(&self) -> String {
        match self.status {
            RunStatus::Running => "in progress".to_string(),
            _ => render_summary(&BuildRunSummary {
                usage: self.usage.clone(),
                resumed_turns: self.resumed_turns,
            }),
        }
    }

    fn prompt_preview(&self) -> String {
        let first_line = self.prompt.lines().next().unwrap_or("").trim();
        if first_line.is_empty() {
            return "(empty prompt)".to_string();
        }
        let preview: String = first_line.chars().take(40).collect();
        if first_line.chars().count() > 40 {
            format!("{preview}...")
        } else {
            preview
        }
    }
}

#[derive(Debug, Clone)]
struct BuildWorkspace {
    name: String,
    path: PathBuf,
    git: WorkspaceGitSnapshot,
    runs: Vec<BuildRunEntry>,
    selected_run: usize,
}

impl BuildWorkspace {
    fn selected_run(&self) -> Option<&BuildRunEntry> {
        self.runs.get(self.selected_run)
    }

    fn row_label(&self, is_running: bool) -> String {
        if is_running {
            format!("{} | {} | active", self.name, self.git.status_label())
        } else {
            format!("{} | {}", self.name, self.git.status_label())
        }
    }
}

enum BuildEvent {
    Output(String),
    Complete {
        usage: Option<AgentTokenUsage>,
        continuation: Option<AgentContinuation>,
        sync_summary: String,
    },
    Failed {
        error: String,
        sync_summary: String,
    },
}

struct AgentRunResult {
    usage: Option<AgentTokenUsage>,
    continuation: Option<AgentContinuation>,
}

struct BuildExecutionContext {
    config: AppConfig,
    planning_meta: PlanningMeta,
    args: BuildArgs,
    workspace_dir: PathBuf,
    interrupt_flag: Arc<AtomicBool>,
    event_tx: mpsc::Sender<BuildEvent>,
}

struct BuildDashboardApp {
    workspace_root: PathBuf,
    config: AppConfig,
    planning_meta: PlanningMeta,
    args: BuildArgs,
    workspaces: Vec<BuildWorkspace>,
    workspace_index: usize,
    focus: BuildFocus,
    prompt: InputFieldState,
    output_scroll: ScrollState,
    status_scroll: ScrollState,
    copy: CopyUiState,
    agent_running: bool,
    active_workspace: Option<usize>,
    pending_continuation: Option<AgentContinuation>,
    current_run_before_snapshot: Option<WorkspaceGitSnapshot>,
    current_output_bytes: usize,
    run_started_at: Option<Instant>,
    interrupt_flag: Arc<AtomicBool>,
    event_tx: mpsc::Sender<BuildEvent>,
    event_rx: mpsc::Receiver<BuildEvent>,
    sticky_status: Option<String>,
    last_refresh_at: Instant,
}

impl BuildDashboardApp {
    fn new(
        workspace_root: PathBuf,
        config: AppConfig,
        planning_meta: PlanningMeta,
        args: BuildArgs,
        workspaces: Vec<BuildWorkspace>,
        workspace_index: usize,
    ) -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        Self {
            workspace_root,
            config,
            planning_meta,
            args,
            workspaces,
            workspace_index,
            focus: BuildFocus::Workspaces,
            prompt: InputFieldState::multiline(String::new()),
            output_scroll: ScrollState::default(),
            status_scroll: ScrollState::default(),
            copy: CopyUiState::default(),
            agent_running: false,
            active_workspace: None,
            pending_continuation: None,
            current_run_before_snapshot: None,
            current_output_bytes: 0,
            run_started_at: None,
            interrupt_flag: Arc::new(AtomicBool::new(false)),
            event_tx,
            event_rx,
            sticky_status: None,
            last_refresh_at: Instant::now(),
        }
    }

    fn selected_workspace(&self) -> Option<&BuildWorkspace> {
        self.workspaces.get(self.workspace_index)
    }

    fn selected_workspace_mut(&mut self) -> Option<&mut BuildWorkspace> {
        self.workspaces.get_mut(self.workspace_index)
    }

    fn provider_label(&self, prompt: &str) -> Result<String> {
        let invocation =
            resolve_build_invocation(&self.config, &self.planning_meta, &self.args, prompt)?;
        Ok(provider_display(
            &invocation.agent,
            invocation.model.as_deref(),
        ))
    }

    fn refresh_visible_workspaces(&mut self) {
        if self.last_refresh_at.elapsed() < Duration::from_millis(800) {
            return;
        }
        self.last_refresh_at = Instant::now();

        let mut indexes = BTreeSet::new();
        if !self.workspaces.is_empty() {
            indexes.insert(self.workspace_index);
        }
        if let Some(active) = self.active_workspace {
            indexes.insert(active);
        }

        for index in indexes {
            if let Some(workspace) = self.workspaces.get_mut(index)
                && let Ok(snapshot) = inspect_workspace_git(&workspace.path)
            {
                workspace.git = snapshot;
            }
        }
    }

    fn submit_prompt(&mut self) -> Result<()> {
        if self.agent_running {
            return Ok(());
        }
        if self.total_run_count() >= self.args.max_turns {
            self.sticky_status = Some(format!(
                "maximum run count reached ({})",
                self.args.max_turns
            ));
            return Ok(());
        }

        let workspace_index = self
            .selected_workspace_index()
            .ok_or_else(|| anyhow!("select a workspace before launching a build run"))?;
        let prompt = self.prompt.value().trim().to_string();
        if prompt.is_empty() {
            self.sticky_status = Some("enter a prompt before launching a build run".to_string());
            return Ok(());
        }

        let provider_label = self.provider_label(&prompt)?;
        let invocation =
            resolve_build_invocation(&self.config, &self.planning_meta, &self.args, &prompt)?;
        let continuation = self
            .pending_continuation
            .take()
            .filter(|state| state.provider == invocation.agent && invocation.builtin_provider);

        let before_snapshot = inspect_workspace_git(&self.workspaces[workspace_index].path)
            .with_context(|| {
                format!(
                    "failed to inspect workspace `{}` before launching",
                    self.workspaces[workspace_index].name
                )
            })?;
        self.workspaces[workspace_index].git = before_snapshot.clone();
        let run_number = self.workspaces[workspace_index].runs.len() as u32 + 1;
        self.workspaces[workspace_index].runs.push(BuildRunEntry {
            number: run_number,
            prompt: prompt.clone(),
            provider_label,
            status: RunStatus::Running,
            output: String::new(),
            usage: None,
            resumed_turns: u32::from(continuation.is_some()),
            sync_summary: "Sync pending...".to_string(),
            change_summary: "Waiting for workspace changes...".to_string(),
            publish_summary: "Publish pending...".to_string(),
        });
        self.workspaces[workspace_index].selected_run = self.workspaces[workspace_index]
            .runs
            .len()
            .saturating_sub(1);
        self.prompt.clear();
        self.output_scroll.reset();
        self.agent_running = true;
        self.active_workspace = Some(workspace_index);
        self.current_run_before_snapshot = Some(before_snapshot);
        self.current_output_bytes = 0;
        self.run_started_at = Some(Instant::now());
        self.interrupt_flag.store(false, Ordering::SeqCst);
        self.focus = BuildFocus::Output;
        self.sticky_status = Some(format!(
            "launching run #{} in {}",
            run_number, self.workspaces[workspace_index].name
        ));

        spawn_agent_thread(
            BuildExecutionContext {
                config: self.config.clone(),
                planning_meta: self.planning_meta.clone(),
                args: self.args.clone(),
                workspace_dir: self.workspaces[workspace_index].path.clone(),
                interrupt_flag: Arc::clone(&self.interrupt_flag),
                event_tx: self.event_tx.clone(),
            },
            prompt,
            continuation,
        );
        Ok(())
    }

    fn drain_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                BuildEvent::Output(text) => {
                    self.current_output_bytes += text.len();
                    if let Some(workspace) = self.active_workspace_mut()
                        && let Some(run) = workspace.runs.last_mut()
                    {
                        run.output.push_str(&text);
                    }
                    self.scroll_output_to_bottom();
                }
                BuildEvent::Complete {
                    usage,
                    continuation,
                    sync_summary,
                } => {
                    self.complete_active_run(RunStatus::Success, usage, continuation, sync_summary);
                }
                BuildEvent::Failed {
                    error,
                    sync_summary,
                } => {
                    let status = if self.interrupt_flag.load(Ordering::SeqCst) {
                        RunStatus::Interrupted
                    } else {
                        RunStatus::Failure(error.clone())
                    };
                    self.complete_active_run(status, None, None, sync_summary);
                    self.sticky_status = Some(error);
                }
            }
        }
    }

    fn complete_active_run(
        &mut self,
        status: RunStatus,
        usage: Option<AgentTokenUsage>,
        continuation: Option<AgentContinuation>,
        sync_summary: String,
    ) {
        let active_workspace = self.active_workspace;
        self.agent_running = false;
        self.pending_continuation = continuation;
        self.run_started_at = None;
        self.current_output_bytes = 0;

        if let Some(index) = active_workspace {
            let after_snapshot = inspect_workspace_git(&self.workspaces[index].path)
                .unwrap_or_else(|_| self.workspaces[index].git.clone());
            let before_snapshot = self.current_run_before_snapshot.take().unwrap_or_default();
            let change_summary = summarize_change_delta(&before_snapshot, &after_snapshot);
            let publish_summary = if matches!(status, RunStatus::Success) {
                publish_workspace_changes(
                    &self.workspaces[index].path,
                    &after_snapshot,
                    self.workspaces[index]
                        .runs
                        .last()
                        .map(|run| run.prompt.as_str())
                        .unwrap_or("update workspace"),
                )
                .unwrap_or_else(|error| format!("publish failed: {error}"))
            } else {
                "publish skipped".to_string()
            };
            self.workspaces[index].git =
                inspect_workspace_git(&self.workspaces[index].path).unwrap_or(after_snapshot);
            if let Some(run) = self.workspaces[index].runs.last_mut() {
                run.status = status;
                run.usage = usage;
                run.sync_summary = sync_summary;
                run.change_summary = change_summary;
                run.publish_summary = publish_summary;
            }
        }

        self.active_workspace = None;
        if self.sticky_status.is_none() {
            self.sticky_status = Some("run completed".to_string());
        }
    }

    fn active_workspace_mut(&mut self) -> Option<&mut BuildWorkspace> {
        let index = self.active_workspace?;
        self.workspaces.get_mut(index)
    }

    fn selected_workspace_index(&self) -> Option<usize> {
        (!self.workspaces.is_empty()).then_some(self.workspace_index)
    }

    fn total_run_count(&self) -> u32 {
        self.workspaces
            .iter()
            .map(|workspace| workspace.runs.len() as u32)
            .sum()
    }

    fn scroll_output_to_bottom(&mut self) {
        let Some(workspace) = self.selected_workspace() else {
            return;
        };
        let Some(run) = workspace.selected_run() else {
            return;
        };
        let viewport = output_viewport(Rect::new(0, 0, 140, 40));
        let rows = wrapped_rows(&run.output, viewport.width.max(1)).max(1);
        let bottom = rows.saturating_sub(1) as u16;
        let _ = self
            .output_scroll
            .ensure_row_visible(bottom, viewport.height.max(1), rows);
    }

    fn interrupt(&mut self) {
        if self.agent_running {
            self.interrupt_flag.store(true, Ordering::SeqCst);
            self.sticky_status = Some("interrupt requested".to_string());
        }
    }

    fn elapsed_label(&self) -> Option<String> {
        self.run_started_at
            .map(|started| format!("{}s", started.elapsed().as_secs()))
    }

    fn copy_payload(&self) -> CopyPayload {
        match self.focus {
            BuildFocus::Workspaces => {
                let text = if self.workspaces.is_empty() {
                    "No workspace clones found.".to_string()
                } else {
                    self.workspaces
                        .iter()
                        .map(|workspace| {
                            format!(
                                "{} | {} | {}",
                                workspace.name,
                                workspace.git.branch,
                                workspace.git.status_label()
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                CopyPayload::from_text("build workspaces", text.into())
            }
            BuildFocus::Runs => {
                let text = self
                    .selected_workspace()
                    .map(|workspace| {
                        if workspace.runs.is_empty() {
                            "No runs for the selected workspace.".to_string()
                        } else {
                            workspace
                                .runs
                                .iter()
                                .map(|run| {
                                    format!(
                                        "#{} [{}] {} | {}",
                                        run.number,
                                        run.status.label(),
                                        run.provider_label,
                                        run.prompt_preview()
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n")
                        }
                    })
                    .unwrap_or_else(|| "No workspace selected.".to_string());
                CopyPayload::from_text("build runs", text.into())
            }
            BuildFocus::Output => self
                .selected_workspace()
                .and_then(BuildWorkspace::selected_run)
                .map(|run| {
                    CopyPayload::from_text(
                        format!("build run {} output", run.number),
                        run.output.clone().into(),
                    )
                })
                .unwrap_or_else(|| {
                    CopyPayload::from_text("build output", "No run selected.".into())
                }),
            BuildFocus::Status => {
                CopyPayload::from_text("workspace status", plain_status_text(self).into())
            }
            BuildFocus::Prompt => self.prompt.copy_payload("build prompt"),
        }
    }
}

pub(crate) struct BuildDashboardOptions {
    pub(crate) source_root: PathBuf,
    pub(crate) workspace_root: PathBuf,
    pub(crate) initial_workspace: Option<PathBuf>,
    pub(crate) config: AppConfig,
    pub(crate) planning_meta: PlanningMeta,
    pub(crate) args: BuildArgs,
    pub(crate) initial_prompt: Option<String>,
}

/// Runs the interactive `agents build` dashboard for one or more resolved workspaces.
///
/// Returns an error when stdout is not a TTY, terminal setup fails, workspace discovery fails, or
/// a provider subprocess cannot be launched for a submitted prompt.
pub(crate) fn run_build_dashboard(options: BuildDashboardOptions) -> Result<()> {
    if !io::stdout().is_terminal() {
        bail!(
            "the interactive build dashboard requires a TTY; use `{} agents build --no-interactive` for scripted runs",
            branding::COMMAND_NAME
        );
    }

    let (workspaces, workspace_index) = load_dashboard_workspaces(
        &options.source_root,
        &options.workspace_root,
        options.initial_workspace.as_deref(),
    )?;

    enable_raw_mode().context("failed to enable raw mode for build dashboard")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .context("failed to enter build dashboard screen")?;
    let _cleanup = TerminalCleanup;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal =
        Terminal::new(backend).context("failed to initialize build dashboard terminal")?;
    let mut app = BuildDashboardApp::new(
        options.workspace_root,
        options.config,
        options.planning_meta,
        options.args,
        workspaces,
        workspace_index,
    );

    if let Some(prompt) = options.initial_prompt {
        app.prompt = InputFieldState::multiline(prompt);
        app.submit_prompt()?;
    }

    loop {
        app.drain_events();
        app.refresh_visible_workspaces();
        terminal
            .draw(|frame| render_dashboard(frame, &app))
            .context("failed to render build dashboard")?;

        if !event::poll(Duration::from_millis(100)).context("failed to poll build dashboard")? {
            continue;
        }

        match event::read().context("failed to read build dashboard event")? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if is_mouse_toggle_key(key) {
                    app.copy.toggle_mouse_capture(terminal.backend_mut())?;
                    continue;
                }

                let size = terminal.size().context("failed to read terminal size")?;
                if app.copy.export_active()
                    && app
                        .copy
                        .handle_export_key(key, copy_overlay_viewport(size.into()))
                {
                    continue;
                }

                if is_copy_key(key) {
                    app.copy.copy_payload(app.copy_payload());
                    continue;
                }

                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    if app.agent_running {
                        app.interrupt();
                    } else {
                        break;
                    }
                    continue;
                }

                match app.focus {
                    BuildFocus::Workspaces => handle_workspace_keys(&mut app, key),
                    BuildFocus::Runs => handle_run_keys(&mut app, key),
                    BuildFocus::Output => {
                        handle_output_keys(&mut app, key, output_viewport(size.into()))
                    }
                    BuildFocus::Status => {
                        handle_status_keys(&mut app, key, status_viewport(size.into()))
                    }
                    BuildFocus::Prompt => {
                        handle_prompt_keys(&mut app, key, prompt_viewport(size.into()))?
                    }
                }
            }
            Event::Paste(text) if app.focus == BuildFocus::Prompt => {
                let _ = app.prompt.paste(&text);
            }
            Event::Mouse(mouse)
                if matches!(
                    mouse.kind,
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                ) =>
            {
                let size = terminal.size().context("failed to read terminal size")?;
                if app.copy.export_active() {
                    let _ = app
                        .copy
                        .handle_export_mouse(mouse, copy_overlay_viewport(size.into()));
                } else if app.focus == BuildFocus::Output {
                    if let Some(workspace) = app.selected_workspace()
                        && let Some(run) = workspace.selected_run()
                    {
                        let viewport = output_viewport(size.into());
                        let rows = wrapped_rows(&run.output, viewport.width.max(1)).max(1);
                        let _ = app
                            .output_scroll
                            .apply_mouse_in_viewport(mouse, viewport, rows);
                    }
                } else if app.focus == BuildFocus::Status {
                    let viewport = status_viewport(size.into());
                    let rows = wrapped_rows(&plain_status_text(&app), viewport.width.max(1)).max(1);
                    let _ = app
                        .status_scroll
                        .apply_mouse_in_viewport(mouse, viewport, rows);
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn handle_workspace_keys(app: &mut BuildDashboardApp, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Tab => app.focus = BuildFocus::Runs,
        KeyCode::Enter => app.focus = BuildFocus::Prompt,
        KeyCode::Up if app.workspace_index > 0 => {
            app.workspace_index -= 1;
            app.output_scroll.reset();
        }
        KeyCode::Down if app.workspace_index + 1 < app.workspaces.len() => {
            app.workspace_index += 1;
            app.output_scroll.reset();
        }
        KeyCode::Home if !app.workspaces.is_empty() => {
            app.workspace_index = 0;
            app.output_scroll.reset();
        }
        KeyCode::End if !app.workspaces.is_empty() => {
            app.workspace_index = app.workspaces.len() - 1;
            app.output_scroll.reset();
        }
        _ => {}
    }
}

fn handle_run_keys(app: &mut BuildDashboardApp, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Tab => app.focus = BuildFocus::Output,
        KeyCode::Esc => app.focus = BuildFocus::Workspaces,
        KeyCode::Up => {
            if let Some(workspace) = app.selected_workspace_mut()
                && workspace.selected_run > 0
            {
                workspace.selected_run -= 1;
                app.output_scroll.reset();
            }
        }
        KeyCode::Down => {
            if let Some(workspace) = app.selected_workspace_mut()
                && workspace.selected_run + 1 < workspace.runs.len()
            {
                workspace.selected_run += 1;
                app.output_scroll.reset();
            }
        }
        KeyCode::Home => {
            if let Some(workspace) = app.selected_workspace_mut()
                && !workspace.runs.is_empty()
            {
                workspace.selected_run = 0;
                app.output_scroll.reset();
            }
        }
        KeyCode::End => {
            if let Some(workspace) = app.selected_workspace_mut()
                && !workspace.runs.is_empty()
            {
                workspace.selected_run = workspace.runs.len() - 1;
                app.output_scroll.reset();
            }
        }
        _ => {}
    }
}

fn handle_output_keys(
    app: &mut BuildDashboardApp,
    key: crossterm::event::KeyEvent,
    viewport: Rect,
) {
    match key.code {
        KeyCode::Tab => app.focus = BuildFocus::Status,
        KeyCode::Esc => app.focus = BuildFocus::Runs,
        _ => {
            if let Some(workspace) = app.selected_workspace()
                && let Some(run) = workspace.selected_run()
            {
                let rows = wrapped_rows(&run.output, viewport.width.max(1)).max(1);
                let _ = app.output_scroll.apply_key_in_viewport(key, viewport, rows);
            }
        }
    }
}

fn handle_status_keys(
    app: &mut BuildDashboardApp,
    key: crossterm::event::KeyEvent,
    viewport: Rect,
) {
    match key.code {
        KeyCode::Tab => app.focus = BuildFocus::Prompt,
        KeyCode::Esc => app.focus = BuildFocus::Output,
        _ => {
            let rows = wrapped_rows(&plain_status_text(app), viewport.width.max(1)).max(1);
            let _ = app.status_scroll.apply_key_in_viewport(key, viewport, rows);
        }
    }
}

fn handle_prompt_keys(
    app: &mut BuildDashboardApp,
    key: crossterm::event::KeyEvent,
    viewport: Rect,
) -> Result<()> {
    if key.code == KeyCode::Enter && !key.modifiers.contains(KeyModifiers::SHIFT) {
        return app.submit_prompt();
    }
    match key.code {
        KeyCode::Tab => app.focus = BuildFocus::Workspaces,
        KeyCode::Esc => app.focus = BuildFocus::Output,
        _ => {
            let _ = app.prompt.handle_key_with_viewport(
                key,
                viewport.width.max(1),
                viewport.height.max(1),
            );
        }
    }
    Ok(())
}

fn render_dashboard(frame: &mut Frame<'_>, app: &BuildDashboardApp) {
    let area = frame.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(12),
            Constraint::Length(6),
            Constraint::Length(2),
        ])
        .split(area);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(29),
            Constraint::Percentage(27),
            Constraint::Percentage(44),
        ])
        .split(outer[1]);
    let detail = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(body[2]);

    render_header(frame, outer[0], app);
    render_workspaces(frame, body[0], app);
    render_runs(frame, body[1], app);
    render_output(frame, detail[0], app);
    render_workspace_status(frame, detail[1], app);
    render_prompt(frame, outer[2], app);
    render_footer(frame, outer[3], app);
    app.copy.render_export_overlay(frame, area);
}

fn render_header(frame: &mut Frame<'_>, area: Rect, app: &BuildDashboardApp) {
    let provider_line = match app.provider_label(app.prompt.value()) {
        Ok(provider) => provider,
        Err(_) => "provider resolution pending".to_string(),
    };
    let mut status_line = vec![
        Span::styled("Workspace root ", muted_style()),
        Span::styled(app.workspace_root.display().to_string(), emphasis_style()),
        Span::raw("  "),
        Span::styled("Provider ", muted_style()),
        Span::raw(provider_line),
        Span::raw("  "),
        badge(
            if app.agent_running { "running" } else { "idle" },
            if app.agent_running {
                Tone::Info
            } else {
                Tone::Muted
            },
        ),
    ];
    if let Some(active) = app.active_workspace
        && let Some(workspace) = app.workspaces.get(active)
    {
        status_line.push(Span::raw("  "));
        status_line.push(Span::raw(format!("active {}", workspace.name)));
    }
    if app.pending_continuation.is_some() {
        status_line.push(Span::raw("  "));
        status_line.push(badge("resume-ready", Tone::Accent));
    }

    let text = Text::from(vec![
        Line::from(status_line),
        Line::from(format!(
            "{} workspace(s), {} total run(s)",
            app.workspaces.len(),
            app.total_run_count()
        )),
        key_hints(&[
            ("Up/Down", "select"),
            ("Tab", "cycle panes"),
            ("Enter", "choose/send"),
            ("Shift+Enter", "newline"),
            ("PgUp/PgDn", "scroll output/status"),
            ("Ctrl+C", "stop/quit"),
            ("Ctrl+Y", "copy"),
        ]),
    ]);
    frame.render_widget(
        Paragraph::new(text).block(
            Block::default()
                .title(panel_title(
                    format!("{} agents build", branding::COMMAND_NAME),
                    false,
                ))
                .borders(Borders::ALL),
        ),
        area,
    );
}

fn render_workspaces(frame: &mut Frame<'_>, area: Rect, app: &BuildDashboardApp) {
    let block = Block::default()
        .title(panel_title(
            "Workspaces",
            app.focus == BuildFocus::Workspaces,
        ))
        .borders(Borders::ALL)
        .border_style(if app.focus == BuildFocus::Workspaces {
            emphasis_style()
        } else {
            Style::default()
        });

    if app.workspaces.is_empty() {
        frame.render_widget(
            Paragraph::new(format!(
                "No workspace clones found under `{}`.\nUse `{} agents listen` to create them or run `{} agents build --dir <PATH>` for a direct checkout.",
                app.workspace_root.display(),
                branding::COMMAND_NAME,
                branding::COMMAND_NAME
            ))
            .block(block)
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }

    let items = app
        .workspaces
        .iter()
        .enumerate()
        .map(|(index, workspace)| {
            let is_running = app.active_workspace == Some(index);
            ListItem::new(Text::from(vec![
                Line::from(vec![
                    badge(workspace.name.clone(), Tone::Accent),
                    Span::raw(" "),
                    Span::styled(workspace.git.branch.clone(), muted_style()),
                ]),
                Line::from(workspace.row_label(is_running)),
            ]))
        })
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");
    let mut state = ListState::default();
    state.select((!app.workspaces.is_empty()).then_some(app.workspace_index));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_runs(frame: &mut Frame<'_>, area: Rect, app: &BuildDashboardApp) {
    let selected_name = app
        .selected_workspace()
        .map(|workspace| workspace.name.clone())
        .unwrap_or_else(|| "Runs".to_string());
    let block = Block::default()
        .title(panel_title(
            format!("Runs - {selected_name}"),
            app.focus == BuildFocus::Runs,
        ))
        .borders(Borders::ALL)
        .border_style(if app.focus == BuildFocus::Runs {
            emphasis_style()
        } else {
            Style::default()
        });

    let Some(workspace) = app.selected_workspace() else {
        frame.render_widget(
            Paragraph::new("Select a workspace to review its build history.")
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    };

    if workspace.runs.is_empty() {
        frame.render_widget(
            Paragraph::new(
                "No runs yet for this workspace.\nMove to the prompt pane and submit a request.",
            )
            .block(block)
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }

    let items = workspace
        .runs
        .iter()
        .map(|run| {
            ListItem::new(Text::from(vec![
                Line::from(vec![
                    badge(format!("#{}", run.number), Tone::Accent),
                    Span::raw(" "),
                    badge(run.status.label(), run.status.tone()),
                ]),
                Line::from(run.prompt_preview()),
                Line::from(vec![
                    Span::styled(run.summary(), muted_style()),
                    Span::raw("  "),
                    Span::styled(run.change_summary.clone(), muted_style()),
                ]),
                Line::from(Span::styled(run.publish_summary.clone(), muted_style())),
            ]))
        })
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");
    let mut state = ListState::default();
    state.select(Some(workspace.selected_run));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_output(frame: &mut Frame<'_>, area: Rect, app: &BuildDashboardApp) {
    let (title, body) = match app
        .selected_workspace()
        .and_then(BuildWorkspace::selected_run)
    {
        Some(run) => {
            let mut title = format!("Output - Run #{} [{}]", run.number, run.status.label());
            if app.agent_running
                && let Some(elapsed) = app.elapsed_label()
            {
                title.push_str(&format!(" {elapsed}"));
            }
            let body = if run.output.is_empty() {
                if matches!(run.status, RunStatus::Running) {
                    "Waiting for agent output...".to_string()
                } else {
                    "No output captured.".to_string()
                }
            } else {
                run.output.clone()
            };
            (title, body)
        }
        None => (
            "Output".to_string(),
            "Select a workspace and a run to inspect agent output.".to_string(),
        ),
    };

    let block = Block::default()
        .title(panel_title(title, app.focus == BuildFocus::Output))
        .borders(Borders::ALL)
        .border_style(if app.focus == BuildFocus::Output {
            emphasis_style()
        } else {
            Style::default()
        })
        .padding(Padding::new(1, 1, 0, 0));
    let paragraph = scrollable_paragraph_with_block(Text::from(body), block, &app.output_scroll)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_workspace_status(frame: &mut Frame<'_>, area: Rect, app: &BuildDashboardApp) {
    let block = Block::default()
        .title(panel_title(
            "Workspace Status",
            app.focus == BuildFocus::Status,
        ))
        .borders(Borders::ALL)
        .border_style(if app.focus == BuildFocus::Status {
            emphasis_style()
        } else {
            Style::default()
        })
        .padding(Padding::new(1, 1, 0, 0));
    let paragraph =
        scrollable_paragraph_with_block(workspace_status_text(app), block, &app.status_scroll)
            .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_prompt(frame: &mut Frame<'_>, area: Rect, app: &BuildDashboardApp) {
    let selected_workspace = app
        .selected_workspace()
        .map(|workspace| workspace.name.clone())
        .unwrap_or_else(|| "No workspace selected".to_string());
    let title = if app.agent_running {
        format!("Prompt -> {} [agent running]", selected_workspace)
    } else {
        format!("Prompt -> {}", selected_workspace)
    };
    let block = Block::default()
        .title(panel_title(title, app.focus == BuildFocus::Prompt))
        .borders(Borders::ALL)
        .border_style(if app.focus == BuildFocus::Prompt {
            emphasis_style()
        } else {
            Style::default()
        });
    let inner = block.inner(area);
    let rendered = app.prompt.render_with_viewport(
        if app.workspaces.is_empty() {
            "No workspaces available. Use --dir or create sibling workspace clones first."
        } else if app.agent_running {
            "The agent is running. Use Ctrl+C to interrupt."
        } else {
            "Describe the update for the selected workspace. Enter submits, Shift+Enter inserts a newline."
        },
        app.focus == BuildFocus::Prompt && !app.agent_running && !app.workspaces.is_empty(),
        inner.width.max(1),
        inner.height.max(1),
    );
    frame.render_widget(rendered.paragraph(block), area);
    if app.focus == BuildFocus::Prompt && !app.agent_running && !app.workspaces.is_empty() {
        rendered.set_cursor(frame, inner);
    }
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, app: &BuildDashboardApp) {
    let text =
        app.copy
            .status_text()
            .map(str::to_string)
            .unwrap_or_else(|| {
                app.sticky_status.clone().unwrap_or_else(|| {
            if app.workspaces.is_empty() {
                "No workspace clones found. Launch with --dir for a direct checkout.".to_string()
            } else if app.agent_running {
                "Agent running. Watch output and workspace status for live change confirmation."
                    .to_string()
            } else if app.pending_continuation.is_some() {
                "Next compatible run can resume the previous Codex session.".to_string()
            } else {
                "Select a workspace, review its history and status, then send the next update request."
                    .to_string()
            }
        })
            });
    frame.render_widget(Paragraph::new(pane_copy_help(&text)), area);
}

fn load_dashboard_workspaces(
    source_root: &Path,
    workspace_root: &Path,
    initial_workspace: Option<&Path>,
) -> Result<(Vec<BuildWorkspace>, usize)> {
    if let Some(workspace) = initial_workspace {
        let git = inspect_workspace_git(workspace)?;
        let name = workspace
            .file_name()
            .and_then(|value| value.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| workspace.display().to_string());
        return Ok((
            vec![BuildWorkspace {
                name,
                path: workspace.to_path_buf(),
                git,
                runs: Vec::new(),
                selected_run: 0,
            }],
            0,
        ));
    }

    let mut workspaces = Vec::new();
    let entries = match fs::read_dir(workspace_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read `{}`", workspace_root.display()));
        }
    };

    for entry in entries {
        let entry =
            entry.with_context(|| format!("failed to read `{}`", workspace_root.display()))?;
        if !entry
            .file_type()
            .with_context(|| format!("failed to inspect `{}`", entry.path().display()))?
            .is_dir()
        {
            continue;
        }
        let path = entry.path();
        if !path.join(".git").exists() {
            continue;
        }
        ensure_workspace_path_is_safe(source_root, workspace_root, &path)?;
        let git = inspect_workspace_git(&path)
            .with_context(|| format!("failed to inspect workspace `{}`", path.display()))?;
        let name = entry
            .file_name()
            .to_str()
            .map(str::to_string)
            .unwrap_or_else(|| path.display().to_string());
        workspaces.push(BuildWorkspace {
            name,
            path,
            git,
            runs: Vec::new(),
            selected_run: 0,
        });
    }

    workspaces.sort_by(|left, right| left.name.cmp(&right.name));
    Ok((workspaces, 0))
}

fn inspect_workspace_git(workspace_path: &Path) -> Result<WorkspaceGitSnapshot> {
    let branch = git_stdout(workspace_path, &["rev-parse", "--abbrev-ref", "HEAD"])
        .context("failed to inspect the workspace branch")?;
    let status = git_stdout(workspace_path, &["status", "--porcelain"])
        .context("failed to inspect local workspace changes")?;
    let has_unpushed_commits = workspace_has_unpushed_commits(workspace_path)?;

    let mut snapshot = WorkspaceGitSnapshot {
        branch: branch.clone(),
        has_unpushed_commits,
        is_detached: branch == "HEAD",
        ..WorkspaceGitSnapshot::default()
    };

    for line in status.lines().filter(|line| !line.trim().is_empty()) {
        let (left, right, path) = parse_porcelain_line(line);
        if path.is_empty() {
            continue;
        }
        snapshot.changed_files.push(path);
        if left == '?' && right == '?' {
            snapshot.untracked_count += 1;
            continue;
        }
        if left == 'R' || right == 'R' {
            snapshot.renamed_count += 1;
        }
        if left == 'D' || right == 'D' {
            snapshot.deleted_count += 1;
        }
        if matches!((left, right), ('U', _) | (_, 'U'))
            || matches!((left, right), ('A', 'A') | ('D', 'D'))
        {
            snapshot.conflicted_count += 1;
        }
        if !matches!(left, ' ' | '?' | 'D' | 'R') || !matches!(right, ' ' | '?' | 'D' | 'R') {
            snapshot.modified_count += 1;
        }
    }
    snapshot.changed_files.sort();
    snapshot.changed_files.dedup();
    Ok(snapshot)
}

fn parse_porcelain_line(line: &str) -> (char, char, String) {
    let mut chars = line.chars();
    let left = chars.next().unwrap_or(' ');
    let right = chars.next().unwrap_or(' ');
    let remainder = line.get(3..).unwrap_or("").trim();
    let path = remainder
        .split(" -> ")
        .last()
        .unwrap_or(remainder)
        .trim()
        .to_string();
    (left, right, path)
}

fn summarize_change_delta(before: &WorkspaceGitSnapshot, after: &WorkspaceGitSnapshot) -> String {
    let before_files = before
        .changed_files
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let after_files = after.changed_files.iter().cloned().collect::<BTreeSet<_>>();
    let added = after_files
        .difference(&before_files)
        .cloned()
        .collect::<Vec<_>>();
    let removed = before_files
        .difference(&after_files)
        .cloned()
        .collect::<Vec<_>>();

    if added.is_empty() && removed.is_empty() && before_files == after_files {
        if after.clean() {
            return "no workspace changes detected".to_string();
        }
        return format!(
            "workspace remains dirty with {} changed file(s)",
            after.changed_files.len()
        );
    }

    let mut parts = Vec::new();
    if after.clean() {
        parts.push("workspace ended clean".to_string());
    } else {
        parts.push(format!(
            "workspace now has {} changed file(s)",
            after.changed_files.len()
        ));
    }
    if !added.is_empty() {
        parts.push(format!(
            "{} new during this run{}",
            added.len(),
            render_file_excerpt(&added)
        ));
    }
    if !removed.is_empty() {
        parts.push(format!("{} cleared during this run", removed.len()));
    }
    parts.join(", ")
}

fn workspace_status_text(app: &BuildDashboardApp) -> Text<'static> {
    if let Some(workspace) = app.selected_workspace() {
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Workspace ", muted_style()),
                Span::styled(workspace.name.clone(), emphasis_style()),
            ]),
            Line::from(vec![
                Span::styled("Path ", muted_style()),
                Span::raw(workspace.path.display().to_string()),
            ]),
        ];
        for line in workspace.git.detail_lines() {
            lines.push(Line::from(line));
        }
        if let Some(run) = workspace.selected_run() {
            lines.push(Line::from(String::new()));
            lines.push(Line::from(vec![
                Span::styled("Sync ", muted_style()),
                Span::raw(run.sync_summary.clone()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Last run ", muted_style()),
                Span::raw(run.change_summary.clone()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Run status ", muted_style()),
                Span::raw(run.status.label().to_string()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Publish ", muted_style()),
                Span::raw(run.publish_summary.clone()),
            ]));
            if matches!(run.status, RunStatus::Running) {
                lines.push(Line::from(vec![
                    Span::styled("Live output ", muted_style()),
                    Span::raw(format!("{} bytes streamed", app.current_output_bytes)),
                ]));
            }
        }
        if !workspace.git.changed_files.is_empty() {
            lines.push(Line::from(String::new()));
            lines.push(Line::from(Span::styled("Changed files:", emphasis_style())));
            for file in &workspace.git.changed_files {
                lines.push(Line::from(format!("- {file}")));
            }
        }
        Text::from(lines)
    } else {
        Text::from("No workspace selected.")
    }
}

fn plain_status_text(app: &BuildDashboardApp) -> String {
    workspace_status_text(app)
        .lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_file_excerpt(files: &[String]) -> String {
    let preview = files.iter().take(3).cloned().collect::<Vec<_>>();
    if preview.is_empty() {
        String::new()
    } else if files.len() > preview.len() {
        format!(
            " ({}, +{} more)",
            preview.join(", "),
            files.len() - preview.len()
        )
    } else {
        format!(" ({})", preview.join(", "))
    }
}

fn sync_workspace_before_run(context: &BuildExecutionContext) -> Result<String> {
    let workspace_path = context.workspace_dir.as_path();
    let snapshot = inspect_workspace_git(workspace_path)?;
    if snapshot.is_detached {
        bail!("sync aborted because the workspace HEAD is detached");
    }
    if !snapshot.clean() {
        bail!(
            "sync aborted because `{}` has local file changes; commit, push, or clean the workspace before running another build",
            workspace_path.display()
        );
    }

    let branch = snapshot.branch.trim();
    if branch.is_empty() || branch == "HEAD" {
        bail!("sync aborted because the workspace branch could not be resolved");
    }

    send_build_output(
        &context.event_tx,
        format!("[sync] Fetching latest refs for `{branch}` from origin...\n"),
    );
    run_git(workspace_path, &["fetch", "--prune", "origin"])
        .with_context(|| format!("failed to fetch latest changes for `{branch}`"))?;

    let Some(upstream) = resolve_workspace_upstream(workspace_path, branch)? else {
        send_build_output(
            &context.event_tx,
            format!("[sync] No upstream branch configured for `{branch}`. Skipping pull.\n"),
        );
        return Ok(format!("sync skipped; `{branch}` has no upstream branch"));
    };

    let before_head = git_stdout(workspace_path, &["rev-parse", "--short", "HEAD"])?;
    let before_upstream = git_stdout(workspace_path, &["rev-parse", "--short", &upstream])?;
    if before_head == before_upstream {
        send_build_output(
            &context.event_tx,
            format!("[sync] `{branch}` is already current with `{upstream}`.\n"),
        );
        return Ok(format!(
            "already current with `{upstream}` at {before_head}"
        ));
    }

    send_build_output(
        &context.event_tx,
        format!("[sync] Rebasing `{branch}` onto `{upstream}`...\n"),
    );
    rebase_workspace_with_agent_assistance(context, branch, &upstream)?;
    let after_head = git_stdout(workspace_path, &["rev-parse", "--short", "HEAD"])?;
    send_build_output(
        &context.event_tx,
        format!("[sync] Workspace synced. HEAD is now `{after_head}` on `{branch}`.\n"),
    );
    Ok(format!(
        "rebased `{branch}` onto `{upstream}` ({before_head} -> {after_head})"
    ))
}

fn rebase_workspace_with_agent_assistance(
    context: &BuildExecutionContext,
    branch: &str,
    upstream: &str,
) -> Result<()> {
    let workspace_path = context.workspace_dir.as_path();
    let mut continue_rebase = false;
    loop {
        if context.interrupt_flag.load(Ordering::SeqCst) {
            bail!("agent interrupted by user");
        }

        let rebase_command = if continue_rebase {
            vec!["rebase", "--continue"]
        } else {
            vec!["rebase", upstream]
        };
        let rebase_result =
            run_git_with_env(workspace_path, &rebase_command, &[("GIT_EDITOR", "true")]);
        match rebase_result {
            Ok(()) => {
                if rebase_in_progress(workspace_path)? {
                    continue_rebase = true;
                    continue;
                }
                return Ok(());
            }
            Err(error) => {
                let conflicted_files =
                    git_stdout(workspace_path, &["diff", "--name-only", "--diff-filter=U"])
                        .unwrap_or_default();
                if conflicted_files.trim().is_empty() {
                    return Err(error)
                        .with_context(|| format!("failed to rebase `{branch}` onto `{upstream}`"));
                }

                send_build_output(
                    &context.event_tx,
                    format!(
                        "[sync] Rebase conflict detected in {}. Launching agent assistance...\n",
                        conflicted_files.replace('\n', ", ")
                    ),
                );
                let resolution_prompt = build_rebase_conflict_prompt(
                    workspace_path,
                    branch,
                    upstream,
                    conflicted_files.trim(),
                )?;
                let _ = run_build_prompt_subprocess(context, &resolution_prompt, None)?;
                run_git(workspace_path, &["add", "-A"])
                    .context("failed to stage agent conflict resolution changes")?;
                let unresolved =
                    git_stdout(workspace_path, &["diff", "--name-only", "--diff-filter=U"])?;
                if !unresolved.trim().is_empty() {
                    bail!(
                        "rebase conflict remains unresolved after agent assistance: {}",
                        unresolved
                    );
                }

                send_build_output(
                    &context.event_tx,
                    "[sync] Conflict edits staged. Continuing rebase...\n".to_string(),
                );
                continue_rebase = true;
            }
        }
    }
}

fn build_rebase_conflict_prompt(
    workspace_path: &Path,
    branch: &str,
    upstream: &str,
    conflicted_files: &str,
) -> Result<String> {
    let head = git_stdout(workspace_path, &["rev-parse", "--short", "HEAD"])?;
    Ok(format!(
        "Resolve an in-progress git rebase conflict inside `{}`.\nBranch: `{}`\nTarget upstream: `{}`\nCurrent HEAD: `{}`\nConflicted files:\n{}\n\nEdit the workspace in place, stage the resolved files, and leave the repository ready for `git rebase --continue`. Then print a short Markdown summary of what you changed.",
        workspace_path.display(),
        branch,
        upstream,
        head,
        conflicted_files
    ))
}

fn resolve_workspace_upstream(workspace_path: &Path, branch: &str) -> Result<Option<String>> {
    if let Ok(upstream) = git_stdout(
        workspace_path,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    ) {
        return Ok(Some(upstream));
    }

    let origin_ref = format!("refs/remotes/origin/{branch}");
    if !git_succeeds(
        workspace_path,
        &["show-ref", "--verify", "--quiet", &origin_ref],
    )? {
        return Ok(None);
    }

    let upstream = format!("origin/{branch}");
    run_git(
        workspace_path,
        &["branch", "--set-upstream-to", &upstream, branch],
    )
    .with_context(|| format!("failed to set upstream for `{branch}`"))?;
    Ok(Some(upstream))
}

fn rebase_in_progress(workspace_path: &Path) -> Result<bool> {
    let git_dir = git_dir_path(workspace_path)?;
    Ok(git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists())
}

fn git_dir_path(workspace_path: &Path) -> Result<PathBuf> {
    let git_dir = git_stdout(workspace_path, &["rev-parse", "--git-dir"])?;
    let path = PathBuf::from(&git_dir);
    Ok(if path.is_absolute() {
        path
    } else {
        workspace_path.join(path)
    })
}

fn send_build_output(event_tx: &mpsc::Sender<BuildEvent>, text: String) {
    let _ = event_tx.send(BuildEvent::Output(text));
}

fn git_stdout(root: &Path, args: &[&str]) -> Result<String> {
    let output = git_output(root, args, &[])?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run_git(root: &Path, args: &[&str]) -> Result<()> {
    let _ = git_output(root, args, &[])?;
    Ok(())
}

fn run_git_with_env(root: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<()> {
    let _ = git_output(root, args, env)?;
    Ok(())
}

fn git_succeeds(root: &Path, args: &[&str]) -> Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .with_context(|| format!("failed to run `git {}`", args.join(" ")))?;
    Ok(output.status.success())
}

fn git_output(root: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<std::process::Output> {
    let mut command = Command::new("git");
    command.arg("-C").arg(root).args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command
        .output()
        .with_context(|| format!("failed to run `git {}`", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output)
}

fn publish_workspace_changes(
    workspace_path: &Path,
    after_snapshot: &WorkspaceGitSnapshot,
    prompt: &str,
) -> Result<String> {
    if after_snapshot.clean() {
        return Ok("no publish needed; workspace is clean".to_string());
    }
    if after_snapshot.is_detached {
        bail!("publish skipped because the workspace HEAD is detached");
    }
    let branch = after_snapshot.branch.trim();
    if branch.is_empty() || branch == "HEAD" {
        bail!("publish skipped because the workspace branch could not be resolved");
    }

    run_git(workspace_path, &["add", "-A"])?;
    let staged = git_stdout(workspace_path, &["diff", "--cached", "--name-only"])?;
    if staged.trim().is_empty() {
        return Ok("no publish needed; nothing staged after git add".to_string());
    }

    let commit_message = build_commit_message(prompt);
    run_git(workspace_path, &["commit", "-m", &commit_message])
        .with_context(|| "failed to commit workspace changes")?;
    let commit_sha = git_stdout(workspace_path, &["rev-parse", "--short", "HEAD"])?;

    let has_upstream = git_stdout(
        workspace_path,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
    .is_ok();

    if has_upstream {
        run_git(workspace_path, &["push", "--force-with-lease"])
            .with_context(|| format!("failed to push branch `{branch}`"))?;
    } else {
        run_git(
            workspace_path,
            &[
                "push",
                "--set-upstream",
                "origin",
                branch,
                "--force-with-lease",
            ],
        )
        .with_context(|| format!("failed to push branch `{branch}` to origin"))?;
    }

    Ok(format!(
        "committed {commit_sha} and pushed `{branch}` with --force-with-lease"
    ))
}

fn build_commit_message(prompt: &str) -> String {
    let first_line = prompt.lines().next().unwrap_or("update workspace").trim();
    let suffix: String = if first_line.is_empty() {
        "update workspace".to_string()
    } else {
        first_line.chars().take(60).collect()
    };
    format!("meta agents build: {suffix}")
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
    let ahead = match upstream {
        Ok(_) => git_stdout(
            workspace_path,
            &["rev-list", "--count", "--left-only", "@{upstream}...HEAD"],
        ),
        Err(_) => git_stdout(workspace_path, &["rev-list", "--count", "HEAD"]),
    }?;
    Ok(ahead.parse::<u64>().unwrap_or(0) > 0)
}

fn prompt_viewport(area: Rect) -> Rect {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(12),
            Constraint::Length(6),
            Constraint::Length(2),
        ])
        .split(area);
    Rect {
        x: outer[2].x.saturating_add(1),
        y: outer[2].y.saturating_add(1),
        width: outer[2].width.saturating_sub(2),
        height: outer[2].height.saturating_sub(2),
    }
}

fn output_viewport(area: Rect) -> Rect {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(12),
            Constraint::Length(6),
            Constraint::Length(2),
        ])
        .split(area);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(29),
            Constraint::Percentage(27),
            Constraint::Percentage(44),
        ])
        .split(outer[1]);
    let detail = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(body[2]);
    Rect {
        x: detail[0].x.saturating_add(2),
        y: detail[0].y.saturating_add(1),
        width: detail[0].width.saturating_sub(4),
        height: detail[0].height.saturating_sub(2),
    }
}

fn status_viewport(area: Rect) -> Rect {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(12),
            Constraint::Length(6),
            Constraint::Length(2),
        ])
        .split(area);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(29),
            Constraint::Percentage(27),
            Constraint::Percentage(44),
        ])
        .split(outer[1]);
    let detail = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(body[2]);
    Rect {
        x: detail[1].x.saturating_add(2),
        y: detail[1].y.saturating_add(1),
        width: detail[1].width.saturating_sub(4),
        height: detail[1].height.saturating_sub(2),
    }
}

fn spawn_agent_thread(
    context: BuildExecutionContext,
    prompt: String,
    continuation: Option<AgentContinuation>,
) {
    thread::spawn(move || {
        let sync_summary = match sync_workspace_before_run(&context) {
            Ok(summary) => summary,
            Err(error) => {
                let _ = context.event_tx.send(BuildEvent::Failed {
                    error: error.to_string(),
                    sync_summary: format!("sync failed: {error}"),
                });
                return;
            }
        };

        let result = run_build_prompt_subprocess(&context, &prompt, continuation.as_ref());
        match result {
            Ok(result) => {
                let _ = context.event_tx.send(BuildEvent::Complete {
                    usage: result.usage,
                    continuation: result.continuation,
                    sync_summary,
                });
            }
            Err(error) => {
                let _ = context.event_tx.send(BuildEvent::Failed {
                    error: error.to_string(),
                    sync_summary,
                });
            }
        }
    });
}

fn run_build_prompt_subprocess(
    context: &BuildExecutionContext,
    prompt: &str,
    continuation: Option<&AgentContinuation>,
) -> Result<AgentRunResult> {
    let invocation = resolve_build_invocation(
        &context.config,
        &context.planning_meta,
        &context.args,
        prompt,
    )?;
    run_agent_subprocess(
        &invocation,
        prompt,
        context.workspace_dir.as_path(),
        continuation,
        &context.interrupt_flag,
        &context.event_tx,
    )
}

fn run_agent_subprocess(
    invocation: &crate::agents::resolution::ResolvedAgentInvocation,
    prompt: &str,
    workspace_dir: &Path,
    continuation: Option<&AgentContinuation>,
    interrupt_flag: &Arc<AtomicBool>,
    event_tx: &mpsc::Sender<BuildEvent>,
) -> Result<AgentRunResult> {
    let continuation_session_id = continuation
        .filter(|state| state.provider == invocation.agent)
        .map(|state| state.session_id.clone());
    let capture_output = invocation.builtin_provider && invocation.agent == "codex";
    let command_args = command_args_for_invocation_with_options(
        invocation,
        AgentExecutionOptions {
            working_dir: Some(workspace_dir.to_path_buf()),
            extra_env: Vec::new(),
            capture_output,
            continuation: continuation_session_id,
        },
    )?;
    let attempted = validate_invocation_command_surface(invocation, &command_args)?;

    let mut command = Command::new(&invocation.command);
    command.args(&command_args);
    command.current_dir(workspace_dir);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    apply_noninteractive_agent_environment(&mut command);
    apply_invocation_environment(&mut command, invocation, prompt, None);

    let mut child: Child = command.spawn().with_context(|| {
        format!(
            "failed to launch agent `{}` with command `{attempted}`",
            invocation.agent
        )
    })?;

    if invocation.transport == PromptTransport::Stdin {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("failed to open stdin for agent `{}`", invocation.agent))?;
        stdin
            .write_all(invocation.payload.as_bytes())
            .with_context(|| {
                format!(
                    "failed to write prompt payload to agent `{}`",
                    invocation.agent
                )
            })?;
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("failed to open stdout for agent `{}`", invocation.agent))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("failed to open stderr for agent `{}`", invocation.agent))?;
    let (output_tx, output_rx) = mpsc::channel::<String>();
    spawn_reader(stdout, output_tx.clone());
    spawn_reader(stderr, output_tx);

    let mut raw_stdout = String::new();
    loop {
        match output_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                raw_stdout.push_str(&chunk);
                let _ = event_tx.send(BuildEvent::Output(chunk));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if interrupt_flag.load(Ordering::SeqCst) {
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!("agent interrupted by user");
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    if interrupt_flag.load(Ordering::SeqCst) {
        let _ = child.kill();
        let _ = child.wait();
        bail!("agent interrupted by user");
    }

    let status = child
        .wait()
        .with_context(|| format!("failed to wait for agent `{}`", invocation.agent))?;

    let mut usage = None;
    let mut continuation = None;
    if capture_output
        && let Some(provider) = builtin_provider_adapter(&invocation.agent)
        && let Ok(parsed) = provider.parse_capture_output(&raw_stdout)
    {
        usage = parsed.usage;
        continuation = parsed.continuation.map(|session_id| AgentContinuation {
            provider: invocation.agent.clone(),
            session_id,
        });
    }

    if !status.success() {
        let code = status
            .code()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "terminated by signal".to_string());
        bail!(
            "agent `{}` exited unsuccessfully ({code}) while running `{attempted}`",
            invocation.agent
        );
    }

    Ok(AgentRunResult {
        usage,
        continuation,
    })
}

fn spawn_reader(mut reader: impl Read + Send + 'static, output_tx: mpsc::Sender<String>) {
    thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    let text = String::from_utf8_lossy(&buffer[..count]).to_string();
                    if output_tx.send(text).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

struct TerminalCleanup;

impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, DisableMouseCapture, LeaveAlternateScreen);
    }
}

#[cfg(test)]
fn render_dashboard_snapshot(app: BuildDashboardApp, width: u16, height: u16) -> Result<String> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|frame| render_dashboard(frame, &app))?;
    Ok(format!("{}", terminal.backend()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> BuildDashboardApp {
        BuildDashboardApp::new(
            PathBuf::from("/tmp/repo-workspace"),
            AppConfig::default(),
            PlanningMeta::default(),
            BuildArgs {
                positionals: Vec::new(),
                root: PathBuf::from("."),
                agent: None,
                model: None,
                reasoning: None,
                dir: None,
                max_turns: 20,
                no_interactive: false,
            },
            vec![BuildWorkspace {
                name: "MET-45".to_string(),
                path: PathBuf::from("/tmp/repo-workspace/MET-45"),
                git: WorkspaceGitSnapshot {
                    branch: "feature/met-45".to_string(),
                    changed_files: vec!["src/lib.rs".to_string()],
                    modified_count: 1,
                    deleted_count: 0,
                    renamed_count: 0,
                    untracked_count: 0,
                    conflicted_count: 0,
                    has_unpushed_commits: false,
                    is_detached: false,
                },
                runs: Vec::new(),
                selected_run: 0,
            }],
            0,
        )
    }

    #[test]
    fn change_delta_mentions_new_files() {
        let before = WorkspaceGitSnapshot::default();
        let after = WorkspaceGitSnapshot {
            changed_files: vec!["src/lib.rs".to_string(), "README.md".to_string()],
            modified_count: 2,
            ..WorkspaceGitSnapshot::default()
        };
        let summary = summarize_change_delta(&before, &after);
        assert!(summary.contains("workspace now has 2 changed file(s)"));
        assert!(summary.contains("new during this run"));
    }

    #[test]
    fn render_snapshot_shows_workspace_inventory() {
        let app = test_app();
        let snapshot = render_dashboard_snapshot(app, 140, 36).expect("snapshot");
        assert!(snapshot.contains("Workspaces"));
        assert!(snapshot.contains("MET-45"));
        assert!(snapshot.contains("Workspace Status"));
    }

    #[test]
    fn render_snapshot_shows_run_change_summary() {
        let mut app = test_app();
        app.workspaces[0].runs.push(BuildRunEntry {
            number: 1,
            prompt: "fix auth".to_string(),
            provider_label: "codex (gpt-5.4)".to_string(),
            status: RunStatus::Success,
            output: "updated src/lib.rs".to_string(),
            usage: Some(AgentTokenUsage {
                input: Some(50),
                output: Some(25),
            }),
            resumed_turns: 0,
            sync_summary: "rebased `feature/met-45` onto `origin/feature/met-45`".to_string(),
            change_summary:
                "workspace now has 1 changed file(s), 1 new during this run (src/lib.rs)"
                    .to_string(),
            publish_summary: "committed abc123 and pushed `feature/met-45`".to_string(),
        });
        let snapshot = render_dashboard_snapshot(app, 140, 36).expect("snapshot");
        assert!(snapshot.contains("fix auth"));
        assert!(snapshot.contains("1 changed"));
        assert!(snapshot.contains("pushed"));
    }
}

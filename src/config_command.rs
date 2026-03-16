use std::io;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, anyhow};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use serde::Serialize;

use crate::cli::ConfigArgs;
use crate::config::{
    AppConfig, builtin_agent_definition, detect_supported_agents, normalize_agent_name,
    supported_agent_models, supported_agent_names, validate_agent_model, validate_agent_name,
};
use crate::tui::fields::{InputFieldState, SelectFieldState};

#[derive(Debug, Clone)]
pub struct ConfigReport {
    pub config_path: PathBuf,
    pub changed: bool,
}

#[derive(Debug, Clone)]
pub enum ConfigCommandOutput {
    Text(String),
    Json(String),
}

#[derive(Debug, Clone)]
struct ConfigViewData {
    config_path: PathBuf,
    app_config: AppConfig,
    detected_agents: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SerializableConfigView<'a> {
    config_path: String,
    detected_agents: &'a [String],
    app: &'a AppConfig,
}

pub async fn run_config(args: &ConfigArgs) -> Result<ConfigCommandOutput> {
    let mut view = load_view()?;

    if args.json {
        return Ok(ConfigCommandOutput::Json(render_json(&view)?));
    }

    if has_direct_updates(args) {
        let changed = apply_direct_updates(&mut view, args)?;
        save_view(&view)?;
        return Ok(ConfigCommandOutput::Text(
            ConfigReport {
                config_path: view.config_path.clone(),
                changed,
            }
            .render(&view),
        ));
    }

    if args.render_once {
        return Ok(ConfigCommandOutput::Text(render_once(
            ConfigApp::new(&view),
            args,
        )?));
    }

    if io::stdin().is_terminal() && io::stdout().is_terminal() {
        let exit = run_config_dashboard(ConfigApp::new(&view))?;
        return match exit {
            ConfigDashboardExit::Cancelled => Ok(ConfigCommandOutput::Text(
                "Configuration dashboard cancelled.".to_string(),
            )),
            ConfigDashboardExit::Submitted(submitted) => {
                submitted.apply(&mut view)?;
                save_view(&view)?;
                Ok(ConfigCommandOutput::Text(
                    ConfigReport {
                        config_path: view.config_path.clone(),
                        changed: true,
                    }
                    .render(&view),
                ))
            }
        };
    }

    Ok(ConfigCommandOutput::Text(render_summary(&view, false)))
}

impl ConfigReport {
    fn render(&self, view: &ConfigViewData) -> String {
        let verb = if self.changed { "saved" } else { "unchanged" };
        format!(
            "Configuration {verb}. Config: {}.\n{}",
            self.config_path.display(),
            render_summary(view, true)
        )
    }
}

fn load_view() -> Result<ConfigViewData> {
    Ok(ConfigViewData {
        config_path: crate::config::resolve_config_path()?,
        app_config: AppConfig::load()?,
        detected_agents: detect_supported_agents(),
    })
}

fn save_view(view: &ConfigViewData) -> Result<()> {
    view.app_config.save()?;
    Ok(())
}

fn render_json(view: &ConfigViewData) -> Result<String> {
    Ok(serde_json::to_string_pretty(&SerializableConfigView {
        config_path: view.config_path.display().to_string(),
        detected_agents: &view.detected_agents,
        app: &view.app_config,
    })?)
}

fn render_summary(view: &ConfigViewData, include_path: bool) -> String {
    let mut lines = Vec::new();
    if include_path {
        lines.push(format!("Config path: {}", view.config_path.display()));
    }
    lines.push(format!(
        "Linear API key: {}",
        mask_secret(view.app_config.linear.api_key.as_deref())
    ));
    lines.push(format!(
        "Default Linear team: {}",
        display_optional(view.app_config.linear.team.as_deref())
    ));
    lines.push(format!(
        "Default Linear profile: {}",
        display_optional(view.app_config.linear.default_profile.as_deref())
    ));
    lines.push(format!(
        "Default agent: {}",
        display_optional(view.app_config.agents.default_agent.as_deref())
    ));
    lines.push(format!(
        "Default model: {}",
        display_optional(view.app_config.agents.default_model.as_deref())
    ));
    lines.push(format!(
        "Default reasoning: {}",
        display_optional(view.app_config.agents.default_reasoning.as_deref())
    ));
    lines.push(format!(
        "Configured Linear profiles: {}",
        if view.app_config.linear.profiles.is_empty() {
            "none".to_string()
        } else {
            view.app_config
                .linear
                .profiles
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        }
    ));
    lines.push(format!(
        "Detected agents on PATH: {}",
        if view.detected_agents.is_empty() {
            "none".to_string()
        } else {
            view.detected_agents.join(", ")
        }
    ));
    lines.join("\n")
}

fn display_optional(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unset")
        .to_string()
}

fn mask_secret(value: Option<&str>) -> String {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.len() <= 6 => "*".repeat(value.len()),
        Some(value) => format!(
            "{}{}",
            "*".repeat(value.len() - 4),
            &value[value.len() - 4..]
        ),
        None => "unset".to_string(),
    }
}

fn has_direct_updates(args: &ConfigArgs) -> bool {
    args.api_key.is_some()
        || args.team.is_some()
        || args.default_profile.is_some()
        || args.default_agent.is_some()
        || args.default_model.is_some()
        || args.default_reasoning.is_some()
}

fn apply_direct_updates(view: &mut ConfigViewData, args: &ConfigArgs) -> Result<bool> {
    let before = serde_json::to_value(&view.app_config)?;

    if let Some(api_key) = &args.api_key {
        view.app_config.linear.api_key = normalize_optional(api_key);
    }
    if let Some(team) = &args.team {
        view.app_config.linear.team = normalize_optional(team);
    }
    if let Some(default_profile) = &args.default_profile {
        let normalized = normalize_optional(default_profile);
        validate_default_profile(&view.app_config, normalized.as_deref())?;
        view.app_config.linear.default_profile = normalized;
    }
    if let Some(default_agent) = &args.default_agent {
        let normalized = normalize_agent_name(default_agent);
        validate_agent_name(&view.app_config, &normalized)?;
        view.app_config.agents.default_agent = Some(normalized.clone());
        if let Some(definition) = builtin_agent_definition(&normalized) {
            view.app_config
                .set_agent_definition(&normalized, definition);
        }
        if validate_agent_model(&normalized, view.app_config.agents.default_model.as_deref())
            .is_err()
        {
            view.app_config.agents.default_model = None;
        }
    }
    if let Some(default_model) = &args.default_model {
        let selected_agent = selected_global_agent(&view.app_config);
        let normalized = normalize_optional(default_model);
        validate_agent_model(&selected_agent, normalized.as_deref())?;
        view.app_config.agents.default_model = normalized;
    }
    if let Some(default_reasoning) = &args.default_reasoning {
        view.app_config.agents.default_reasoning = normalize_optional(default_reasoning);
    }

    let after = serde_json::to_value(&view.app_config)?;
    Ok(before != after)
}

fn validate_default_profile(app_config: &AppConfig, profile: Option<&str>) -> Result<()> {
    let Some(profile) = profile else {
        return Ok(());
    };
    if app_config.linear.profiles.contains_key(profile) {
        return Ok(());
    }
    Err(anyhow!(
        "Linear profile `{profile}` is not configured. Add it under `[linear.profiles.{profile}]` before selecting it as the default profile."
    ))
}

fn selected_global_agent(app_config: &AppConfig) -> String {
    app_config
        .agents
        .default_agent
        .clone()
        .unwrap_or_else(|| supported_agent_names()[0].to_string())
}

fn normalize_optional(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigStep {
    ApiKey,
    Team,
    DefaultProfile,
    Agent,
    Model,
    DefaultReasoning,
    Save,
}

impl ConfigStep {
    fn all() -> [Self; 7] {
        [
            Self::ApiKey,
            Self::Team,
            Self::DefaultProfile,
            Self::Agent,
            Self::Model,
            Self::DefaultReasoning,
            Self::Save,
        ]
    }

    fn index(self) -> usize {
        Self::all()
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }

    fn next(self) -> Self {
        let index = (self.index() + 1).min(Self::all().len() - 1);
        Self::all()[index]
    }

    fn previous(self) -> Self {
        let index = self.index().saturating_sub(1);
        Self::all()[index]
    }

    fn label(self) -> &'static str {
        match self {
            Self::ApiKey => "Linear API key",
            Self::Team => "Default team",
            Self::DefaultProfile => "Default profile",
            Self::Agent => "Default agent",
            Self::Model => "Default model",
            Self::DefaultReasoning => "Default reasoning",
            Self::Save => "Save",
        }
    }

    fn panel_label(self) -> &'static str {
        match self {
            Self::ApiKey => "Linear API key",
            Self::Team => "Default Linear team",
            Self::DefaultProfile => "Default Linear profile",
            Self::Agent => "Default agent",
            Self::Model => "Default model",
            Self::DefaultReasoning => "Default reasoning effort",
            Self::Save => "Save configuration",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ConfigAction {
    Up,
    Down,
    Tab,
    BackTab,
    Enter,
    Esc,
}

#[derive(Debug, Clone)]
struct ConfigApp {
    step: ConfigStep,
    api_key: InputFieldState,
    team: InputFieldState,
    default_profile: InputFieldState,
    default_reasoning: InputFieldState,
    agent_field: SelectFieldState,
    model_field: SelectFieldState,
    detected_agents: Vec<String>,
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct SubmittedConfig {
    api_key: Option<String>,
    team: Option<String>,
    default_profile: Option<String>,
    default_agent: String,
    default_model: Option<String>,
    default_reasoning: Option<String>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
enum ConfigDashboardExit {
    Cancelled,
    Submitted(SubmittedConfig),
}

impl ConfigApp {
    fn new(view: &ConfigViewData) -> Self {
        let agent_options = supported_agent_names()
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>();
        let selected_agent = selected_global_agent(&view.app_config);
        let agent_index = agent_options
            .iter()
            .position(|candidate| candidate.eq_ignore_ascii_case(&selected_agent))
            .unwrap_or(0);
        let mut app = Self {
            step: ConfigStep::ApiKey,
            api_key: InputFieldState::new(
                view.app_config.linear.api_key.clone().unwrap_or_default(),
            ),
            team: InputFieldState::new(view.app_config.linear.team.clone().unwrap_or_default()),
            default_profile: InputFieldState::new(
                view.app_config
                    .linear
                    .default_profile
                    .clone()
                    .unwrap_or_default(),
            ),
            default_reasoning: InputFieldState::new(
                view.app_config
                    .agents
                    .default_reasoning
                    .clone()
                    .unwrap_or_default(),
            ),
            agent_field: SelectFieldState::new(agent_options, agent_index),
            model_field: SelectFieldState::new(vec!["Leave unset".to_string()], 0),
            detected_agents: view.detected_agents.clone(),
            error: None,
        };
        app.sync_models(view.app_config.agents.default_model.as_deref());
        app
    }

    fn current_agent(&self) -> &str {
        self.agent_field.selected_label().unwrap_or("codex")
    }

    fn sync_models(&mut self, preferred: Option<&str>) {
        let current = preferred
            .map(str::to_string)
            .or_else(|| self.model_field.selected_label().map(str::to_string))
            .filter(|value| value != "Leave unset");
        let mut options = vec!["Leave unset".to_string()];
        options.extend(
            supported_agent_models(self.current_agent())
                .iter()
                .map(|value| (*value).to_string()),
        );
        let selected = current
            .as_deref()
            .and_then(|value| {
                options
                    .iter()
                    .position(|candidate| candidate.eq_ignore_ascii_case(value))
            })
            .unwrap_or(0);
        self.model_field = SelectFieldState::new(options, selected);
    }

    fn apply_action(&mut self, action: ConfigAction) -> Option<ConfigDashboardExit> {
        match action {
            ConfigAction::Tab => {
                self.step = self.step.next();
                None
            }
            ConfigAction::BackTab => {
                self.step = self.step.previous();
                None
            }
            ConfigAction::Enter => {
                if self.step == ConfigStep::Save {
                    match self.submit() {
                        Ok(submitted) => Some(ConfigDashboardExit::Submitted(submitted)),
                        Err(error) => {
                            self.error = Some(error.to_string());
                            None
                        }
                    }
                } else {
                    self.step = self.step.next();
                    None
                }
            }
            ConfigAction::Esc => Some(ConfigDashboardExit::Cancelled),
            ConfigAction::Up => {
                self.error = None;
                if self.step == ConfigStep::Agent {
                    self.agent_field.move_by(-1);
                    self.sync_models(None);
                } else if self.step == ConfigStep::Model {
                    self.model_field.move_by(-1);
                } else {
                    self.step = self.step.previous();
                }
                None
            }
            ConfigAction::Down => {
                self.error = None;
                if self.step == ConfigStep::Agent {
                    self.agent_field.move_by(1);
                    self.sync_models(None);
                } else if self.step == ConfigStep::Model {
                    self.model_field.move_by(1);
                } else {
                    self.step = self.step.next();
                }
                None
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<ConfigDashboardExit> {
        match key.code {
            KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Left | KeyCode::Right => {
                self.error = None;
                match self.step {
                    ConfigStep::ApiKey => {
                        let _ = self.api_key.handle_key(key);
                    }
                    ConfigStep::Team => {
                        let _ = self.team.handle_key(key);
                    }
                    ConfigStep::DefaultProfile => {
                        let _ = self.default_profile.handle_key(key);
                    }
                    ConfigStep::DefaultReasoning => {
                        let _ = self.default_reasoning.handle_key(key);
                    }
                    ConfigStep::Agent | ConfigStep::Model | ConfigStep::Save => {}
                }
                None
            }
            KeyCode::Up => self.apply_action(ConfigAction::Up),
            KeyCode::Down => self.apply_action(ConfigAction::Down),
            KeyCode::Tab => self.apply_action(ConfigAction::Tab),
            KeyCode::BackTab => self.apply_action(ConfigAction::BackTab),
            KeyCode::Enter => self.apply_action(ConfigAction::Enter),
            KeyCode::Esc => self.apply_action(ConfigAction::Esc),
            _ => None,
        }
    }

    fn handle_paste(&mut self, text: &str) {
        self.error = None;
        match self.step {
            ConfigStep::ApiKey => {
                let _ = self.api_key.paste(text);
            }
            ConfigStep::Team => {
                let _ = self.team.paste(text);
            }
            ConfigStep::DefaultProfile => {
                let _ = self.default_profile.paste(text);
            }
            ConfigStep::DefaultReasoning => {
                let _ = self.default_reasoning.paste(text);
            }
            ConfigStep::Agent | ConfigStep::Model | ConfigStep::Save => {}
        }
    }

    fn submit(&self) -> Result<SubmittedConfig> {
        let default_agent = normalize_agent_name(self.current_agent());
        let default_model = match self.model_field.selected() {
            0 => None,
            _ => self.model_field.selected_label().map(str::to_string),
        };
        validate_agent_name(&AppConfig::load()?, &default_agent)?;
        validate_agent_model(&default_agent, default_model.as_deref())?;
        let app_config = AppConfig::load()?;
        let default_profile = normalize_optional(self.default_profile.value());
        validate_default_profile(&app_config, default_profile.as_deref())?;

        Ok(SubmittedConfig {
            api_key: normalize_optional(self.api_key.value()),
            team: normalize_optional(self.team.value()),
            default_profile,
            default_agent,
            default_model,
            default_reasoning: normalize_optional(self.default_reasoning.value()),
        })
    }
}

impl SubmittedConfig {
    fn apply(&self, view: &mut ConfigViewData) -> Result<()> {
        validate_default_profile(&view.app_config, self.default_profile.as_deref())?;
        validate_agent_name(&view.app_config, &self.default_agent)?;
        validate_agent_model(&self.default_agent, self.default_model.as_deref())?;
        view.app_config.linear.api_key = self.api_key.clone();
        view.app_config.linear.team = self.team.clone();
        view.app_config.linear.default_profile = self.default_profile.clone();
        view.app_config.agents.default_agent = Some(self.default_agent.clone());
        view.app_config.agents.default_model = self.default_model.clone();
        view.app_config.agents.default_reasoning = self.default_reasoning.clone();
        if let Some(definition) = builtin_agent_definition(&self.default_agent) {
            view.app_config
                .set_agent_definition(&self.default_agent, definition);
        }
        Ok(())
    }
}

fn render_once(app: ConfigApp, args: &ConfigArgs) -> Result<String> {
    let backend = TestBackend::new(args.width, args.height);
    let mut terminal = Terminal::new(backend)?;
    let mut app = app;

    for action in args.events.iter().copied().map(ConfigAction::from) {
        if app.apply_action(action).is_some() {
            break;
        }
    }

    terminal.draw(|frame| render_config_dashboard(frame, &app))?;
    Ok(snapshot(terminal.backend()))
}

fn run_config_dashboard(app: ConfigApp) -> Result<ConfigDashboardExit> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;
    let _cleanup = TerminalCleanup;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = app;

    loop {
        terminal.draw(|frame| render_config_dashboard(frame, &app))?;

        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if let Some(exit) = app.handle_key(key) {
                        return Ok(exit);
                    }
                }
                Event::Paste(text) => app.handle_paste(&text),
                _ => {}
            }
        }
    }
}

fn render_config_dashboard(frame: &mut Frame<'_>, app: &ConfigApp) {
    let area = frame.area();
    let header_height = if area.width >= 110 { 5 } else { 6 };
    let footer_height = if area.width >= 96 { 4 } else { 5 };
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(0),
            Constraint::Length(footer_height),
        ])
        .split(area);

    let header = Paragraph::new(Text::from(vec![
        Line::from("Meta Config"),
        Line::from(
            "Configure install-scoped Linear auth plus default agent settings shared across repositories.",
        ),
        Line::from(format!(
            "Detected supported agents on PATH: {}",
            if app.detected_agents.is_empty() {
                "none".to_string()
            } else {
                app.detected_agents.join(", ")
            }
        )),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Global configuration"),
    )
    .wrap(Wrap { trim: false });
    frame.render_widget(header, layout[0]);

    let body_area = layout[1];
    if body_area.width >= 118 {
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(26),
                Constraint::Min(34),
                Constraint::Length(40),
            ])
            .split(body_area);
        render_step_list(frame, app, body[0], 1);
        render_step_panel(frame, app, body[1]);
        render_summary_panel(frame, app, body[2]);
    } else if body_area.width >= 90 {
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(36), Constraint::Min(40)])
            .split(body_area);
        let sidebar = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(9), Constraint::Min(10)])
            .split(body[0]);
        render_step_list(frame, app, sidebar[0], 1);
        render_summary_panel(frame, app, sidebar[1]);
        render_step_panel(frame, app, body[1]);
    } else {
        let stacked = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),
                Constraint::Min(8),
                Constraint::Length(10),
            ])
            .split(body_area);
        render_step_list(frame, app, stacked[0], 2);
        render_step_panel(frame, app, stacked[1]);
        render_summary_panel(frame, app, stacked[2]);
    }
    render_footer(frame, app, layout[2]);
}

fn render_step_list(frame: &mut Frame<'_>, app: &ConfigApp, area: Rect, columns: usize) {
    let block = Block::default().borders(Borders::ALL).title("Steps");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let columns = columns.max(1);
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints((0..columns).map(|_| Constraint::Ratio(1, columns as u32)))
        .split(inner);
    let steps = ConfigStep::all();
    let per_column = steps.len().div_ceil(columns);

    for (column, chunk) in chunks.iter().enumerate() {
        let start = column * per_column;
        let end = (start + per_column).min(steps.len());
        if start >= end {
            continue;
        }

        let lines = steps[start..end]
            .iter()
            .enumerate()
            .map(|(offset, step)| {
                let index = start + offset;
                let selected = index == app.step.index();
                let marker = if selected { "> " } else { "  " };
                let style = if selected {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                Line::from(Span::styled(
                    format!("{marker}{}. {}", index + 1, step.label()),
                    style,
                ))
            })
            .collect::<Vec<_>>();
        let paragraph = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, *chunk);
    }
}

fn render_step_panel(frame: &mut Frame<'_>, app: &ConfigApp, area: Rect) {
    let title = if area.width < 60 {
        app.step.panel_label().to_string()
    } else {
        format!(
            "Step {} of {}: {}",
            app.step.index() + 1,
            ConfigStep::all().len(),
            app.step.panel_label()
        )
    };

    match app.step {
        ConfigStep::ApiKey => render_input_panel(
            frame,
            area,
            &title,
            &app.api_key,
            "Paste the install-scoped Linear API key or leave blank to unset it.",
        ),
        ConfigStep::Team => render_input_panel(
            frame,
            area,
            &title,
            &app.team,
            "Optional default Linear team key used when a command does not set one explicitly.",
        ),
        ConfigStep::DefaultProfile => render_input_panel(
            frame,
            area,
            &title,
            &app.default_profile,
            "Optional default Linear profile name. The profile must already exist under [linear.profiles.<name>].",
        ),
        ConfigStep::Agent => render_select_panel(frame, area, &title, &app.agent_field),
        ConfigStep::Model => render_select_panel(frame, area, &title, &app.model_field),
        ConfigStep::DefaultReasoning => render_input_panel(
            frame,
            area,
            &title,
            &app.default_reasoning,
            "Optional default reasoning effort passed through to supported local agents.",
        ),
        ConfigStep::Save => render_save_panel(frame, area),
    }
}

fn render_summary_panel(frame: &mut Frame<'_>, app: &ConfigApp, area: Rect) {
    let summary = summary_lines(
        area.width,
        &[
            ("Linear API key", summarize_secret(&app.api_key)),
            ("Default team", summarize_optional_value(&app.team)),
            (
                "Default profile",
                summarize_optional_value(&app.default_profile),
            ),
            (
                "Default agent",
                app.agent_field
                    .selected_label()
                    .unwrap_or("unset")
                    .to_string(),
            ),
            (
                "Default model",
                app.model_field
                    .selected_label()
                    .unwrap_or("Leave unset")
                    .to_string(),
            ),
            (
                "Default reasoning",
                summarize_optional_value(&app.default_reasoning),
            ),
            (
                "Detected agents",
                if app.detected_agents.is_empty() {
                    "none".to_string()
                } else {
                    app.detected_agents.join(", ")
                },
            ),
        ],
    );
    let paragraph = Paragraph::new(Text::from(summary))
        .block(Block::default().borders(Borders::ALL).title("Summary"))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_footer(frame: &mut Frame<'_>, app: &ConfigApp, area: Rect) {
    let controls = match app.step {
        ConfigStep::ApiKey
        | ConfigStep::Team
        | ConfigStep::DefaultProfile
        | ConfigStep::DefaultReasoning => {
            "Type or paste the value. Enter or Tab advances. Shift+Tab goes back. Esc cancels."
        }
        ConfigStep::Agent | ConfigStep::Model => {
            "Use Up/Down to choose. Enter or Tab advances. Shift+Tab goes back. Esc cancels."
        }
        ConfigStep::Save => "Press Enter to save. Shift+Tab goes back. Esc cancels.",
    };
    let status = app.error.as_deref().unwrap_or("Ready.");
    let footer = Paragraph::new(Text::from(vec![Line::from(controls), Line::from(status)]))
        .block(Block::default().borders(Borders::ALL).title("Controls"))
        .wrap(Wrap { trim: false });
    frame.render_widget(footer, area);
}

fn render_input_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    field: &InputFieldState,
    placeholder: &str,
) {
    let rendered = field.render(placeholder, true);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("{title} [editing]"))
        .border_style(Style::default().add_modifier(Modifier::BOLD));
    let inner = block.inner(area);
    let paragraph = Paragraph::new(rendered.text.clone())
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
    rendered.set_cursor(frame, inner);
}

fn render_select_panel(frame: &mut Frame<'_>, area: Rect, title: &str, field: &SelectFieldState) {
    let lines = field
        .options()
        .iter()
        .enumerate()
        .map(|(index, option)| {
            let selected = index == field.selected();
            let marker = if selected { "> " } else { "  " };
            let style = if selected {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Line::from(Span::styled(format!("{marker}{option}"), style))
        })
        .collect::<Vec<_>>();
    let list = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false });
    frame.render_widget(list, area);
}

fn render_save_panel(frame: &mut Frame<'_>, area: Rect) {
    let paragraph = Paragraph::new(Text::from(vec![
        Line::from("Review the summary and press Enter to save the install-scoped configuration."),
        Line::from("Repo defaults now live under `meta runtime setup`."),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Save"))
    .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn summarize_secret(field: &InputFieldState) -> String {
    mask_secret(normalize_optional(field.value()).as_deref())
}

fn summarize_optional_value(field: &InputFieldState) -> String {
    normalize_optional(field.value()).unwrap_or_else(|| "unset".to_string())
}

fn summary_lines(width: u16, entries: &[(&str, String)]) -> Vec<Line<'static>> {
    let compact = width < 40;
    let mut lines = Vec::new();

    for (label, value) in entries {
        if compact {
            lines.push(Line::from(Span::styled(
                (*label).to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(format!("  {value}")));
        } else {
            lines.push(Line::from(format!("{label}: {value}")));
        }
    }

    lines
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

struct TerminalCleanup;

impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen);
    }
}

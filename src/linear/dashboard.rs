use std::io::{self, IsTerminal};
use std::time::Duration;

use anyhow::{Result, bail};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use super::browser::{
    IssueSearchResult, empty_search_result, issue_state_label, render_issue_preview,
    render_issue_row, search_issues,
};
use super::{DashboardData, IssueSummary};
use crate::tui::fields::InputFieldState;

#[derive(Debug, Clone)]
pub struct DashboardOptions {
    pub render_once: bool,
    pub width: u16,
    pub height: u16,
    pub actions: Vec<DashboardAction>,
    pub initial_state_filter: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum DashboardAction {
    Up,
    Down,
    Tab,
    Enter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Status,
    Estimate,
    Issues,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EstimateFilter {
    All,
    Unestimated,
    Exact(String),
}

#[derive(Debug, Clone)]
struct FilterOption<T> {
    label: String,
    value: T,
    count: usize,
}

#[derive(Debug, Clone)]
struct DashboardApp {
    data: DashboardData,
    focus: Focus,
    query: InputFieldState,
    status_options: Vec<FilterOption<Option<String>>>,
    estimate_options: Vec<FilterOption<EstimateFilter>>,
    status_index: usize,
    estimate_index: usize,
    issue_index: usize,
    active_status: Option<String>,
    active_estimate: EstimateFilter,
}

pub fn run_dashboard(data: DashboardData, options: DashboardOptions) -> Result<Option<String>> {
    if options.render_once {
        return render_once(data, options).map(Some);
    }

    if !io::stdout().is_terminal() {
        bail!(
            "the interactive issue dashboard requires a TTY; pass `--json` for machine-readable output"
        );
    }

    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;
    let _cleanup = TerminalCleanup;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = DashboardApp::new(data, options.initial_state_filter);

    loop {
        terminal.draw(|frame| render_dashboard(frame, &app))?;

        if event::poll(Duration::from_millis(250))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Up => app.apply(DashboardAction::Up),
                KeyCode::Down => app.apply(DashboardAction::Down),
                KeyCode::Tab => app.apply(DashboardAction::Tab),
                KeyCode::Enter => app.apply(DashboardAction::Enter),
                _ => {
                    let _ = app.handle_query_key(key);
                }
            }
        }
    }

    Ok(None)
}

fn render_once(data: DashboardData, options: DashboardOptions) -> Result<String> {
    let backend = TestBackend::new(options.width, options.height);
    let mut terminal = Terminal::new(backend)?;
    let mut app = DashboardApp::new(data, options.initial_state_filter);
    for action in options.actions {
        app.apply(action);
    }

    terminal.draw(|frame| render_dashboard(frame, &app))?;
    Ok(snapshot(terminal.backend()))
}

fn render_dashboard(frame: &mut Frame<'_>, app: &DashboardApp) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(frame.area());
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(26),
            Constraint::Percentage(34),
            Constraint::Percentage(40),
        ])
        .split(outer[2]);
    let sidebar = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(body[0]);

    let header = Paragraph::new(Text::from(vec![
        Line::from(app.data.title.clone()),
        Line::from(app.summary_line()),
        Line::from("Keys: Type to search while the issue list is focused. Tab changes focus, Up/Down moves selection, Enter applies sidebar filters, q exits."),
    ]))
    .wrap(Wrap { trim: true })
    .block(Block::default().borders(Borders::ALL).title("Linear Issues"));
    frame.render_widget(header, outer[0]);

    let rendered_query = app.query.render(
        "Search by identifier, title, state, project, or description...",
        app.focus == Focus::Issues,
    );
    let query_block = Block::default()
        .borders(Borders::ALL)
        .title(if app.focus == Focus::Issues {
            "Issue Search [active]"
        } else {
            "Issue Search"
        });
    let query_inner = query_block.inner(outer[1]);
    let query = Paragraph::new(rendered_query.text.clone())
        .block(query_block)
        .wrap(Wrap { trim: false });
    frame.render_widget(query, outer[1]);
    rendered_query.set_cursor(frame, query_inner);

    render_filter_list(
        frame,
        sidebar[0],
        "Status",
        app.focus == Focus::Status,
        &app.status_options,
        app.status_index,
        |value| app.status_option_is_active(value),
    );
    render_filter_list(
        frame,
        sidebar[1],
        "Estimate",
        app.focus == Focus::Estimate,
        &app.estimate_options,
        app.estimate_index,
        |value| app.estimate_option_is_active(value),
    );

    let filtered_issue_results = app.visible_issue_results();
    let issue_title = if app.focus == Focus::Issues {
        format!(
            "Issues [focus] ({}/{})",
            filtered_issue_results.len(),
            app.data.issues.len()
        )
    } else {
        format!(
            "Issues ({}/{})",
            filtered_issue_results.len(),
            app.data.issues.len()
        )
    };
    let issue_items = if filtered_issue_results.is_empty() {
        vec![ListItem::new(
            "No issues match the current search and filters.",
        )]
    } else {
        filtered_issue_results
            .iter()
            .filter_map(|result| {
                app.data
                    .issues
                    .get(result.issue_index)
                    .map(|issue| render_issue_row(issue, Some(result), None))
            })
            .collect::<Vec<_>>()
    };
    let mut issue_state = ListState::default();
    if filtered_issue_results.is_empty() {
        issue_state.select(Some(0));
    } else {
        issue_state.select(Some(app.issue_index.min(filtered_issue_results.len() - 1)));
    }
    let issue_list = List::new(issue_items)
        .block(Block::default().borders(Borders::ALL).title(issue_title))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");
    frame.render_stateful_widget(issue_list, body[1], &mut issue_state);

    let preview = Paragraph::new(app.preview_text())
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Description Preview"),
        );
    frame.render_widget(preview, body[2]);
}

fn render_filter_list<T, F>(
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
    title: &str,
    is_focused: bool,
    options: &[FilterOption<T>],
    selected_index: usize,
    is_active: F,
) where
    F: Fn(&T) -> bool,
{
    let mut state = ListState::default();
    state.select(Some(selected_index.min(options.len().saturating_sub(1))));
    let items = options
        .iter()
        .map(|option| {
            let prefix = if is_active(&option.value) {
                "[x]"
            } else {
                "[ ]"
            };
            ListItem::new(format!("{prefix} {} ({})", option.label, option.count))
        })
        .collect::<Vec<_>>();
    let title = if is_focused {
        format!("{title} [focus]")
    } else {
        title.to_string()
    };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, area, &mut state);
}

impl DashboardApp {
    fn new(data: DashboardData, initial_state_filter: Option<String>) -> Self {
        let status_options = build_status_options(&data.issues);
        let estimate_options = build_estimate_options(&data.issues);
        let active_status = initial_state_filter
            .as_deref()
            .and_then(|state| match_status_option(&status_options, state));

        let mut app = Self {
            data,
            focus: Focus::Issues,
            query: InputFieldState::default(),
            status_options,
            estimate_options,
            status_index: 0,
            estimate_index: 0,
            issue_index: 0,
            active_status,
            active_estimate: EstimateFilter::All,
        };

        app.status_index = app.selected_status_index();
        app.estimate_index = app.selected_estimate_index();
        app.clamp_issue_index();
        app
    }

    fn apply(&mut self, action: DashboardAction) {
        match action {
            DashboardAction::Up => self.move_selection(-1),
            DashboardAction::Down => self.move_selection(1),
            DashboardAction::Tab => {
                self.focus = match self.focus {
                    Focus::Status => Focus::Estimate,
                    Focus::Estimate => Focus::Issues,
                    Focus::Issues => Focus::Status,
                };
            }
            DashboardAction::Enter => self.apply_focus_selection(),
        }
    }

    fn move_selection(&mut self, delta: isize) {
        match self.focus {
            Focus::Status => shift_index(&mut self.status_index, self.status_options.len(), delta),
            Focus::Estimate => {
                shift_index(&mut self.estimate_index, self.estimate_options.len(), delta)
            }
            Focus::Issues => {
                let len = self.visible_issue_results().len();
                shift_index(&mut self.issue_index, len, delta);
            }
        }
    }

    fn handle_query_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        if self.focus != Focus::Issues {
            return false;
        }
        if self.query.handle_key(key) {
            self.issue_index = 0;
            return true;
        }
        false
    }

    fn apply_focus_selection(&mut self) {
        match self.focus {
            Focus::Status => {
                if let Some(option) = self.status_options.get(self.status_index) {
                    self.active_status = option.value.clone();
                    self.clamp_issue_index();
                }
            }
            Focus::Estimate => {
                if let Some(option) = self.estimate_options.get(self.estimate_index) {
                    self.active_estimate = option.value.clone();
                    self.clamp_issue_index();
                }
            }
            Focus::Issues => {}
        }
    }

    fn filtered_issue_indices(&self) -> Vec<usize> {
        self.data
            .issues
            .iter()
            .enumerate()
            .filter(|(_, issue)| self.matches_status(issue) && self.matches_estimate(issue))
            .map(|(index, _)| index)
            .collect()
    }

    fn visible_issue_results(&self) -> Vec<IssueSearchResult> {
        let filtered = self.filtered_issue_indices();
        if self.query.value().trim().is_empty() {
            return filtered.into_iter().map(empty_search_result).collect();
        }

        search_issues(&self.data.issues, self.query.value().trim())
            .into_iter()
            .filter(|result| filtered.contains(&result.issue_index))
            .collect()
    }

    fn matches_status(&self, issue: &IssueSummary) -> bool {
        self.active_status
            .as_ref()
            .is_none_or(|status| issue_state_label(issue).eq_ignore_ascii_case(status))
    }

    fn matches_estimate(&self, issue: &IssueSummary) -> bool {
        match &self.active_estimate {
            EstimateFilter::All => true,
            EstimateFilter::Unestimated => issue.estimate.is_none(),
            EstimateFilter::Exact(expected) => issue_estimate_key(issue)
                .as_deref()
                .map(|value| value == expected)
                .unwrap_or(false),
        }
    }

    fn preview_text(&self) -> Text<'static> {
        let results = self.visible_issue_results();
        let Some(selected_result) = results.get(self.issue_index) else {
            return Text::from(vec![
                Line::from("No issues match the current search and filters."),
                Line::from("Adjust the search query or sidebar filters to widen the result set."),
            ]);
        };
        let issue = &self.data.issues[selected_result.issue_index];
        render_issue_preview(
            issue,
            Some(selected_result),
            None,
            "No description provided.",
        )
    }

    fn summary_line(&self) -> String {
        format!(
            "Visible issues: {}/{} | Search: {} | Status: {} | Estimate: {}",
            self.visible_issue_results().len(),
            self.data.issues.len(),
            if self.query.value().trim().is_empty() {
                "all".to_string()
            } else {
                format!("\"{}\"", self.query.value().trim())
            },
            self.active_status.as_deref().unwrap_or("All statuses"),
            match &self.active_estimate {
                EstimateFilter::All => "All estimates".to_string(),
                EstimateFilter::Unestimated => "Unestimated".to_string(),
                EstimateFilter::Exact(value) => format!("{value} pts"),
            }
        )
    }

    fn selected_status_index(&self) -> usize {
        self.status_options
            .iter()
            .position(|option| option.value == self.active_status)
            .unwrap_or(0)
    }

    fn selected_estimate_index(&self) -> usize {
        self.estimate_options
            .iter()
            .position(|option| option.value == self.active_estimate)
            .unwrap_or(0)
    }

    fn status_option_is_active(&self, value: &Option<String>) -> bool {
        self.active_status == *value
    }

    fn estimate_option_is_active(&self, value: &EstimateFilter) -> bool {
        self.active_estimate == *value
    }

    fn clamp_issue_index(&mut self) {
        let len = self.visible_issue_results().len();
        if len == 0 {
            self.issue_index = 0;
        } else {
            self.issue_index = self.issue_index.min(len - 1);
        }
    }
}

fn build_status_options(issues: &[IssueSummary]) -> Vec<FilterOption<Option<String>>> {
    let mut labels = issues.iter().map(issue_state_label).collect::<Vec<_>>();
    labels.sort();
    labels.dedup();

    let mut options = vec![FilterOption {
        label: "All statuses".to_string(),
        value: None,
        count: issues.len(),
    }];
    options.extend(labels.into_iter().map(|label| {
        FilterOption {
            count: issues
                .iter()
                .filter(|issue| issue_state_label(issue) == label)
                .count(),
            value: Some(label.clone()),
            label,
        }
    }));
    options
}

fn build_estimate_options(issues: &[IssueSummary]) -> Vec<FilterOption<EstimateFilter>> {
    let mut values = issues
        .iter()
        .filter_map(issue_estimate_key)
        .collect::<Vec<_>>();
    values.sort_by(|left, right| compare_estimate_keys(left, right));
    values.dedup();

    let mut options = vec![FilterOption {
        label: "All estimates".to_string(),
        value: EstimateFilter::All,
        count: issues.len(),
    }];

    let unestimated_count = issues
        .iter()
        .filter(|issue| issue.estimate.is_none())
        .count();
    if unestimated_count > 0 {
        options.push(FilterOption {
            label: "Unestimated".to_string(),
            value: EstimateFilter::Unestimated,
            count: unestimated_count,
        });
    }

    options.extend(values.into_iter().map(|value| {
        let count = issues
            .iter()
            .filter(|issue| issue_estimate_key(issue).as_deref() == Some(value.as_str()))
            .count();
        FilterOption {
            label: format!("{value} pts"),
            value: EstimateFilter::Exact(value),
            count,
        }
    }));
    options
}

fn match_status_option(options: &[FilterOption<Option<String>>], state: &str) -> Option<String> {
    options.iter().find_map(|option| {
        option
            .value
            .as_ref()
            .filter(|value| value.eq_ignore_ascii_case(state))
            .cloned()
    })
}

fn issue_estimate_key(issue: &IssueSummary) -> Option<String> {
    issue.estimate.map(format_estimate)
}

fn format_estimate(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        let rendered = format!("{value:.2}");
        rendered
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn compare_estimate_keys(left: &str, right: &str) -> std::cmp::Ordering {
    match (left.parse::<f64>(), right.parse::<f64>()) {
        (Ok(left), Ok(right)) => left
            .partial_cmp(&right)
            .unwrap_or(std::cmp::Ordering::Equal),
        _ => left.cmp(right),
    }
}

fn shift_index(index: &mut usize, len: usize, delta: isize) {
    if len == 0 {
        *index = 0;
        return;
    }

    let mut next = *index as isize + delta;
    if next < 0 {
        next = len.saturating_sub(1) as isize;
    } else if next >= len as isize {
        next = 0;
    }
    *index = next as usize;
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

#[cfg(test)]
mod tests {
    use super::{DashboardAction, DashboardApp, EstimateFilter, Focus};
    use crate::linear::DashboardData;
    use crate::tui::fields::InputFieldState;

    #[test]
    fn dashboard_state_applies_status_and_estimate_filters() {
        let mut app = DashboardApp::new(DashboardData::demo(), None);

        assert_eq!(visible_issue_ids(&app), vec!["MET-11", "MET-12"]);

        app.apply(DashboardAction::Tab);
        assert_eq!(app.focus, Focus::Status);
        app.apply(DashboardAction::Down);
        app.apply(DashboardAction::Down);
        app.apply(DashboardAction::Enter);
        assert_eq!(app.active_status.as_deref(), Some("Todo"));
        assert_eq!(visible_issue_ids(&app), vec!["MET-12"]);

        app.apply(DashboardAction::Tab);
        assert_eq!(app.focus, Focus::Estimate);
        app.apply(DashboardAction::Down);
        app.apply(DashboardAction::Down);
        app.apply(DashboardAction::Enter);
        assert_eq!(app.active_estimate, EstimateFilter::Exact("5".to_string()));
        assert_eq!(visible_issue_ids(&app), vec!["MET-12"]);
    }

    #[test]
    fn dashboard_honors_initial_state_filter() {
        let app = DashboardApp::new(DashboardData::demo(), Some("In Progress".to_string()));

        assert_eq!(app.active_status.as_deref(), Some("In Progress"));
        assert_eq!(visible_issue_ids(&app), vec!["MET-11"]);
    }

    #[test]
    fn dashboard_search_filters_visible_issue_results() {
        let mut app = DashboardApp::new(DashboardData::demo(), None);
        app.query = InputFieldState::new("tests");

        assert_eq!(visible_issue_ids(&app), vec!["MET-12"]);
    }

    #[test]
    fn dashboard_search_zero_results_updates_preview_copy() {
        let mut app = DashboardApp::new(DashboardData::demo(), None);
        app.query = InputFieldState::new("zzz");

        assert!(visible_issue_ids(&app).is_empty());
        assert!(
            format!("{:?}", app.preview_text())
                .contains("No issues match the current search and filters.")
        );
    }

    fn visible_issue_ids(app: &DashboardApp) -> Vec<&str> {
        app.visible_issue_results()
            .into_iter()
            .filter_map(|result| app.data.issues.get(result.issue_index))
            .map(|issue| issue.identifier.as_str())
            .collect()
    }
}

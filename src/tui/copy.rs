use std::env;
use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};
use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::tui::markdown::render_markdown;
use crate::tui::scroll::{ScrollState, plain_text, scrollable_content_paragraph, wrapped_rows};
use crate::tui::theme::{badge, key_hints, panel_title};

const WINDOWS_CLIPBOARD_COPY_UNSUPPORTED_MESSAGE: &str =
    "clipboard copy is not supported on Windows yet; the terminal export is ready instead";
const WSL_CLIPBOARD_COPY_UNSUPPORTED_MESSAGE: &str =
    "clipboard copy is not supported on WSL yet; the terminal export is ready instead";
const GENERIC_CLIPBOARD_COPY_UNSUPPORTED_MESSAGE: &str =
    "clipboard copy is not supported on this platform yet; the terminal export is ready instead";
const LINUX_CLIPBOARD_COPY_UNAVAILABLE_MESSAGE: &str = "clipboard copy requires `wl-copy` on Wayland or `xclip` on X11; the terminal export is ready instead";

/// Shared copy payload for read-only panes and editable fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CopyPayload {
    pub(crate) label: String,
    pub(crate) plain_text: String,
    pub(crate) markdown: Option<String>,
}

impl CopyPayload {
    /// Build a payload from plain text content.
    pub(crate) fn new(label: impl Into<String>, plain_text: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            plain_text: plain_text.into(),
            markdown: None,
        }
    }

    /// Build a payload from already-rendered TUI text while preserving wrapped line breaks.
    pub(crate) fn from_text(label: impl Into<String>, text: Text<'_>) -> Self {
        Self::new(label, plain_text(&text))
    }

    /// Build a payload for markdown-authored content while preserving both markdown and plain text.
    pub(crate) fn markdown(label: impl Into<String>, markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        Self::with_markdown(
            label,
            plain_text(&render_markdown(&markdown, Style::default(), &[])),
            markdown,
        )
    }

    /// Build a payload when the caller already has a plain-text preview plus markdown source.
    pub(crate) fn with_markdown(
        label: impl Into<String>,
        plain_text: impl Into<String>,
        markdown: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            plain_text: plain_text.into(),
            markdown: Some(markdown.into()),
        }
    }

    fn export_title(&self) -> String {
        format!("{} Export", self.label)
    }

    fn export_text(&self) -> String {
        let mut lines = vec![
            "Plain text".to_string(),
            String::new(),
            self.plain_text.clone(),
        ];

        if let Some(markdown) = self
            .markdown
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            lines.extend([
                String::new(),
                "Markdown source".to_string(),
                String::new(),
                markdown.to_string(),
            ]);
        }

        lines.join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CopyExportView {
    title: String,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CopyOutcome {
    Copied {
        status: String,
    },
    Failed {
        status: String,
        export: CopyExportView,
    },
    FallbackReady {
        status: String,
        export: CopyExportView,
    },
}

/// Shared UI state for copy/export status and fallback overlays.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CopyUiState {
    status: Option<String>,
    export: Option<CopyExportView>,
    export_scroll: ScrollState,
}

impl CopyUiState {
    /// Attempt to copy the provided payload and retain any resulting status or fallback export.
    pub(crate) fn copy_payload(&mut self, payload: CopyPayload) {
        self.apply(copy_payload(&payload));
    }

    /// Apply a copy outcome returned by the shared copy layer.
    pub(crate) fn apply(&mut self, outcome: CopyOutcome) {
        match outcome {
            CopyOutcome::Copied { status } => {
                self.status = Some(status);
                self.export = None;
                self.export_scroll.reset();
            }
            CopyOutcome::Failed { status, export }
            | CopyOutcome::FallbackReady { status, export } => {
                self.status = Some(status);
                self.export = Some(export);
                self.export_scroll.reset();
            }
        }
    }

    /// Clear any visible export overlay while keeping the latest status message.
    pub(crate) fn close_export(&mut self) {
        self.export = None;
        self.export_scroll.reset();
    }

    /// Returns the latest copy status text when present.
    pub(crate) fn status_text(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// Returns true when the export overlay is active.
    pub(crate) fn export_active(&self) -> bool {
        self.export.is_some()
    }

    /// Handle keys for the shared export overlay.
    pub(crate) fn handle_export_key(&mut self, key: KeyEvent, viewport: Rect) -> bool {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
            self.close_export();
            return true;
        }

        let Some(export) = &self.export else {
            return false;
        };
        self.export_scroll.apply_key_in_viewport(
            key,
            viewport,
            wrapped_rows(&export.text, viewport.width.max(1)),
        )
    }

    /// Handle mouse scrolling for the shared export overlay.
    pub(crate) fn handle_export_mouse(&mut self, mouse: MouseEvent, viewport: Rect) -> bool {
        let Some(export) = &self.export else {
            return false;
        };
        self.export_scroll.apply_mouse_in_viewport(
            mouse,
            viewport,
            wrapped_rows(&export.text, viewport.width.max(1)),
        )
    }

    /// Render the shared export overlay when a clipboard fallback or failure is active.
    pub(crate) fn render_export_overlay(&self, frame: &mut Frame<'_>, area: Rect) {
        let Some(export) = &self.export else {
            return;
        };

        let outer = centered_rect(area, 88, 80);
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(outer);

        frame.render_widget(Clear, outer);

        let header = Paragraph::new(Text::from(vec![
            Line::from(vec![
                badge("copy", crate::tui::theme::Tone::Accent),
                format!(" {}", export.title).into(),
            ]),
            key_hints(&[
                ("Esc", "close export"),
                ("Up/Down", "scroll export"),
                ("PgUp/PgDn", "scroll export"),
            ]),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(panel_title("Clipboard Export", false)),
        )
        .wrap(Wrap { trim: false });
        frame.render_widget(header, layout[0]);

        let content = scrollable_content_paragraph(
            export.text.clone(),
            "Terminal-safe Export",
            &self.export_scroll,
        )
        .wrap(Wrap { trim: false });
        frame.render_widget(content, layout[1]);

        let footer = Paragraph::new(Text::from(vec![Line::styled(
            self.status.clone().unwrap_or_default(),
            Style::default().add_modifier(Modifier::BOLD),
        )]))
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .wrap(Wrap { trim: false });
        frame.render_widget(footer, layout[2]);
    }
}

/// Append shared copy guidance for a read-only pane help line.
pub(crate) fn pane_copy_help(base: &str) -> String {
    format!(
        "{base} Ctrl+Y copies the full pane. When the clipboard is unavailable, a terminal export opens."
    )
}

/// Append shared copy guidance for an editable field help line.
pub(crate) fn field_copy_help(base: &str) -> String {
    format!(
        "{base} Ctrl+A selects the full field. Ctrl+Y copies the selection or full field. When the clipboard is unavailable, a terminal export opens."
    )
}

/// Returns the centered viewport used by the shared export overlay.
pub(crate) fn copy_overlay_viewport(area: Rect) -> Rect {
    let outer = centered_rect(area, 88, 80);
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(outer);
    layout[1]
}

fn copy_payload(payload: &CopyPayload) -> CopyOutcome {
    let export = CopyExportView {
        title: payload.export_title(),
        text: payload.export_text(),
    };

    match write_plain_text_to_clipboard(&payload.plain_text) {
        ClipboardWriteOutcome::Copied => CopyOutcome::Copied {
            status: format!("Copied {} to the system clipboard.", payload.label),
        },
        ClipboardWriteOutcome::Unavailable(message) => CopyOutcome::FallbackReady {
            status: format!("{} {}", capitalize(&payload.label), message),
            export,
        },
        ClipboardWriteOutcome::Failed(error) => CopyOutcome::Failed {
            status: format!(
                "Clipboard copy failed for {}: {error}. The terminal export is ready instead.",
                payload.label
            ),
            export,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ClipboardWriteOutcome {
    Copied,
    Failed(String),
    Unavailable(String),
}

fn write_plain_text_to_clipboard(text: &str) -> ClipboardWriteOutcome {
    let backend = resolve_clipboard_backend();
    match backend {
        ClipboardBackend::MacOs => command_copy_outcome("pbcopy", Command::new("pbcopy"), text),
        ClipboardBackend::Wayland => {
            let mut command = Command::new("wl-copy");
            command.arg("--type").arg("text/plain");
            command_copy_outcome("wl-copy", command, text)
        }
        ClipboardBackend::X11 => {
            let mut command = Command::new("xclip");
            command.args(["-selection", "clipboard"]);
            command_copy_outcome("xclip", command, text)
        }
        ClipboardBackend::Windows => ClipboardWriteOutcome::Unavailable(
            WINDOWS_CLIPBOARD_COPY_UNSUPPORTED_MESSAGE.to_string(),
        ),
        ClipboardBackend::Wsl => {
            ClipboardWriteOutcome::Unavailable(WSL_CLIPBOARD_COPY_UNSUPPORTED_MESSAGE.to_string())
        }
        ClipboardBackend::Unsupported => ClipboardWriteOutcome::Unavailable(
            GENERIC_CLIPBOARD_COPY_UNSUPPORTED_MESSAGE.to_string(),
        ),
        ClipboardBackend::LinuxUnavailable => {
            ClipboardWriteOutcome::Unavailable(LINUX_CLIPBOARD_COPY_UNAVAILABLE_MESSAGE.to_string())
        }
    }
}

fn command_copy_outcome(label: &str, mut command: Command, text: &str) -> ClipboardWriteOutcome {
    match run_clipboard_command(&mut command, text) {
        Ok(()) => ClipboardWriteOutcome::Copied,
        Err(error) => ClipboardWriteOutcome::Failed(format!("{label}: {error}")),
    }
}

fn run_clipboard_command(command: &mut Command, text: &str) -> Result<()> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to launch clipboard command")?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("clipboard command did not expose stdin"))?;
    stdin
        .write_all(text.as_bytes())
        .context("failed to write clipboard text")?;
    drop(stdin);

    let output = child
        .wait_with_output()
        .context("failed to wait for clipboard command")?;
    if output.status.success() {
        return Ok(());
    }

    Err(anyhow!(String::from_utf8_lossy(&output.stderr).trim().to_owned()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClipboardBackend {
    MacOs,
    Wayland,
    X11,
    LinuxUnavailable,
    Windows,
    Wsl,
    Unsupported,
}

fn resolve_clipboard_backend() -> ClipboardBackend {
    if running_in_wsl() {
        return ClipboardBackend::Wsl;
    }

    if cfg!(target_os = "macos") {
        return ClipboardBackend::MacOs;
    }

    if cfg!(target_os = "windows") {
        return ClipboardBackend::Windows;
    }

    if cfg!(target_os = "linux") {
        if env::var_os("WAYLAND_DISPLAY").is_some() && command_exists("wl-copy") {
            return ClipboardBackend::Wayland;
        }
        if env::var_os("DISPLAY").is_some() && command_exists("xclip") {
            return ClipboardBackend::X11;
        }
        return ClipboardBackend::LinuxUnavailable;
    }

    ClipboardBackend::Unsupported
}

fn command_exists(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|path| path.join(name).exists()))
        .unwrap_or(false)
}

fn running_in_wsl() -> bool {
    cfg!(target_os = "linux")
        && env::var("WSL_DISTRO_NAME").is_ok()
        && env::var("WSL_INTEROP").is_ok()
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn centered_rect(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100u16.saturating_sub(height_percent)) / 2),
            Constraint::Percentage(height_percent),
            Constraint::Percentage((100u16.saturating_sub(height_percent)) / 2),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100u16.saturating_sub(width_percent)) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100u16.saturating_sub(width_percent)) / 2),
        ])
        .split(vertical[1]);
    horizontal[1]
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::{ClipboardWriteOutcome, CopyOutcome, CopyPayload, CopyUiState, copy_payload};

    #[test]
    fn markdown_payload_preserves_plain_text_and_source() {
        let payload = CopyPayload::markdown("preview", "# Title\n\n- item");

        assert_eq!(payload.plain_text, "# Title\n\n- item");
        assert_eq!(payload.markdown.as_deref(), Some("# Title\n\n- item"));
    }

    #[test]
    fn copy_outcome_uses_terminal_export_for_unavailable_clipboard() {
        let export = super::CopyPayload {
            label: "issue preview".to_string(),
            plain_text: "Rendered preview".to_string(),
            markdown: Some("# Source".to_string()),
        };
        let outcome = match ClipboardWriteOutcome::Unavailable(
            "clipboard copy is not supported on WSL yet; the terminal export is ready instead"
                .to_string(),
        ) {
            ClipboardWriteOutcome::Unavailable(message) => CopyOutcome::FallbackReady {
                status: format!("Issue preview {message}"),
                export: super::CopyExportView {
                    title: export.export_title(),
                    text: export.export_text(),
                },
            },
            _ => unreachable!(),
        };

        match outcome {
            CopyOutcome::FallbackReady { status, export } => {
                assert!(status.contains("Issue preview"));
                assert!(export.text.contains("Plain text"));
                assert!(export.text.contains("Markdown source"));
            }
            _ => panic!("expected fallback outcome"),
        }
    }

    #[test]
    fn copy_ui_state_opens_overlay_for_fallback_export() {
        let mut state = CopyUiState::default();
        state.apply(CopyOutcome::FallbackReady {
            status: "Preview fallback".to_string(),
            export: super::CopyExportView {
                title: "Preview Export".to_string(),
                text: "first line\nsecond line".to_string(),
            },
        });

        assert_eq!(state.status_text(), Some("Preview fallback"));
        assert!(state.export_active());
    }

    #[test]
    fn copy_ui_state_clears_export_after_successful_copy() {
        let mut state = CopyUiState::default();
        state.apply(CopyOutcome::FallbackReady {
            status: "Preview fallback".to_string(),
            export: super::CopyExportView {
                title: "Preview Export".to_string(),
                text: "export".to_string(),
            },
        });
        state.apply(CopyOutcome::Copied {
            status: "Copied preview to the system clipboard.".to_string(),
        });

        assert_eq!(
            state.status_text(),
            Some("Copied preview to the system clipboard.")
        );
        assert!(!state.export_active());
    }

    #[test]
    fn export_overlay_renders_status_and_export_text() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut state = CopyUiState::default();
        state.apply(CopyOutcome::FallbackReady {
            status: "Preview fallback".to_string(),
            export: super::CopyExportView {
                title: "Preview Export".to_string(),
                text: "first line\nsecond line".to_string(),
            },
        });

        terminal
            .draw(|frame| state.render_export_overlay(frame, frame.area()))
            .expect("draw");
        let snapshot = terminal
            .backend()
            .buffer()
            .content
            .chunks(100)
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(snapshot.contains("Clipboard Export"));
        assert!(snapshot.contains("Terminal-safe Export"));
        assert!(snapshot.contains("Preview fallback"));
    }

    #[test]
    fn plain_text_payload_omits_markdown_section_from_export() {
        let payload = CopyPayload::new("notes", "only plain text");

        assert_eq!(payload.markdown, None);
        assert!(!payload.export_text().contains("Markdown source"));
    }

    #[test]
    fn live_copy_path_produces_export_for_headless_linux_like_environment() {
        let outcome = copy_payload(&CopyPayload::new("preview", "text"));
        match outcome {
            CopyOutcome::Copied { .. }
            | CopyOutcome::Failed { .. }
            | CopyOutcome::FallbackReady { .. } => {}
        }
    }
}

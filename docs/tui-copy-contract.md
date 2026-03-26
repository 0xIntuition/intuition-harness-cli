# Shared TUI Copy Contract

MetaStack interactive terminal flows now share one copy/export contract across read-only panes and
editable fields.

## Contract

- `Ctrl+Y` copies the focused field or pane as plain text.
- When the source content started as markdown, the shared export payload also keeps the markdown
  source so fallback export can show both representations.
- A successful clipboard write shows a stable success status in the active TUI.
- When direct clipboard access is unavailable on the current platform, the TUI opens a
  terminal-safe export overlay with deterministic content and status text instead of silently
  failing.
- Clipboard command failures also surface a deterministic status plus the same export overlay, so
  the content is still recoverable.

The shared implementation lives in:

- `src/tui/copy.rs`
- `src/tui/keybindings.rs`
- `src/tui/fields.rs`
- `src/tui/markdown.rs`
- `src/tui/scroll.rs`

## Audit Matrix

| Surface family | Coverage |
| --- | --- |
| Planning and backlog flows | `src/plan.rs`, `src/backlog_spec.rs`, `src/technical.rs`, `src/backlog_improve.rs` |
| Workflow generation and repo setup forms | `src/workflows.rs`, `src/setup.rs`, `src/config_command.rs`, `src/onboarding.rs` |
| Linear authoring and search flows | `src/linear/create.rs`, `src/linear/edit.rs`, `src/linear/dashboard.rs` |
| Runtime and scheduling forms | `src/cron_dashboard.rs` |
| Sync and workspace dashboards | `src/sync_dashboard.rs`, `src/workspace_dashboard.rs` |
| GitHub and review dashboards | `src/merge_dashboard.rs`, `src/review/dashboard.rs`, `src/review/mod.rs`, `src/improve/dashboard.rs`, `src/improve/mod.rs` |
| Listener dashboards | `src/listen/dashboard.rs`, `src/listen/mod.rs` |

## Notes

- Read-only panes use shared help copy from `pane_copy_help(...)`.
- Editable single-line and multiline controls use shared help copy from `field_copy_help(...)`.
- Export overlays use the same keyboard contract everywhere: `Esc` closes, and standard scroll
  keys continue to work while the overlay is open.

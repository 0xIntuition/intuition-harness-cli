# Implementation Plan

## Workstreams

1. Refactor `src/tui/fields.rs` so rendered text no longer contains a fake caret and the field can distinguish single-line vs multiline paste/edit behavior.
2. Replace duplicated `set_input_cursor` math in the TUI screens with shared wrapped-cursor positioning that respects explicit newlines and wrapped lines.
3. Update plan/technical loading and active-state rendering to use the same vocabulary and stronger visual emphasis.
4. Add targeted unit and snapshot tests for cursor placement, multiline paste, and the refreshed loading/active states.

## Touchpoints

- Shared TUI primitives:
  - `src/tui/fields.rs`
- Interactive dashboards/forms:
  - `src/plan.rs`
  - `src/technical.rs`
  - `src/linear/create.rs`
  - `src/cron_dashboard.rs`
  - `src/setup.rs`
  - `src/config_command.rs`
- Backlog evidence:
  - `.metastack/backlog/MET-167/index.md`
  - `.metastack/backlog/MET-167/checklist.md`
  - `.metastack/backlog/MET-167/validation.md`

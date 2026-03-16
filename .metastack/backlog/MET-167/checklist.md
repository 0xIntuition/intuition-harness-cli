# Checklist: Fix shared TUI input behavior and standardize loading, cursor, and active-state cues across interactive dashboards

Last updated: 2026-03-16

## 1. Reproduction and Scope

- [x] Confirm the shared-input blast radius from in-repo callers of `InputFieldState`.
- [x] Capture the current reproduction signals:
  - shared field rendering injects a literal `|` caret into content
  - pasted multiline text is normalized into one space-separated line
  - wrapped cursor placement is computed from a flat character count only
- [x] Confirm the focused dashboards/forms in scope:
  - `src/plan.rs`
  - `src/technical.rs`
  - `src/linear/create.rs`
  - `src/cron_dashboard.rs`
  - shared render helpers in `src/setup.rs` and `src/config_command.rs`

## 2. Shared Input Model

- [x] Replace inline caret rendering in `src/tui/fields.rs` with cursor metadata that keeps text and cursor separate.
- [x] Preserve newlines for multiline-capable fields while keeping single-line fields normalized.
- [x] Make cursor movement and insertion deterministic for UTF-8 boundaries and line ends.
- [x] Provide shared wrapped-cursor positioning helpers instead of duplicating per-screen math.

## 3. Dashboard Visual Consistency

- [x] Standardize loading copy/title/state cues across `plan` and `technical`.
- [x] Standardize active/focused emphasis for question, answer, and sidebar panels.
- [x] Apply the updated input rendering helpers to all affected forms that use `InputFieldState`.

## 4. Tests and Validation

- [x] Extend `src/tui/fields.rs` tests for multiline paste/render/cursor behavior.
- [x] Add or update targeted snapshot tests for plan/technical/loading-state visuals.
- [x] Add or update targeted tests for create/cron/setup/config cursor and multiline behavior where applicable.
- [x] Run focused Rust tests for the touched modules.

## 5. Exit Criteria

- [x] Acceptance criteria in `index.md` are fully satisfied by code and tests.
- [x] Local backlog notes reflect the final implementation and validation evidence.

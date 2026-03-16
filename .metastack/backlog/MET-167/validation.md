# Validation Plan

## Command Proofs

- `cargo test tui::fields`
- `cargo test plan::tests`
- `cargo test technical::tests`
- `cargo test linear::create::tests`
- `cargo test cron_dashboard::tests`
- `cargo test setup::tests`
- `cargo test config_command::tests`

## Notes

- Pull sync evidence: `origin/main` fetched and `git merge --ff-only origin/main` reported `Already up to date.` at `HEAD 7fe8cc0`.
- Reproduction evidence before edits:
  - `src/tui/fields.rs` renders a literal `|` inside the text buffer for active fields.
  - `InputFieldState::paste` collapses all whitespace, including newlines, into spaces.
  - `plan.rs`, `technical.rs`, `setup.rs`, and `config_command.rs` all derive cursor row/column from a flat `cursor_offset / width` calculation, so explicit line breaks cannot place the cursor correctly.
- Validation evidence after edits:
  - `cargo fmt`
  - `cargo test tui::fields -- --nocapture`
  - `cargo test plan::tests -- --nocapture`
  - `cargo test technical::tests -- --nocapture`
  - `cargo test linear::create::tests -- --nocapture`
  - `cargo test cron_dashboard::tests -- --nocapture`
  - `make quality`
  - PR #77 `quality` failure reproduced locally as `clippy::redundant-pattern-matching` at `src/cron_dashboard.rs:1104`
  - Replaced `assert!(matches!(exit, None))` with `assert!(exit.is_none())`
  - `make quality` rerun passed locally on commit successor to `48de197`
- Environment note:
  - `mix specs.check` could not be executed in this workspace shell because `mix` was unavailable, and `mise exec -- mix specs.check` failed with `No such file or directory`.

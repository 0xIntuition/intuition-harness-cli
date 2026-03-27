# Implementation Plan

## Workstreams

1. Extend the CLI and install-scoped config surface for sequential mode, checkpoint mode, and recent-main limits.
2. Add recent-main GitHub discovery and mode-aware planner artifacts.
3. Split the merge runtime into aggregate and sequential execution paths that share GitHub, validation, and conflict helpers.
4. Add checkpoint rendering and deterministic render-once coverage.
5. Update docs and validation evidence for the new merge contract.

## Touchpoints

- CLI and help text:
  - `src/cli.rs`
- Merge config and config editing:
  - `src/config.rs`
  - `src/config_command.rs`
  - `src/config_resolution.rs`
- GitHub merge metadata and PR publication:
  - `src/github_pr.rs`
- Merge runtime, artifacts, and resume:
  - `src/merge.rs`
- Sequential checkpoint dashboard:
  - `src/merge_dashboard.rs`
- Regression coverage:
  - `tests/config.rs`
  - `tests/merge.rs`
- User-facing docs:
  - `README.md`
  - `docs/merge.md`
  - `docs/workflows-run-tui.md`
  - `validation.md`

## Expected Code Changes

- Add `--sequential` and `--checkpoints` to `meta merge`.
- Add `[merge].recent_main_limit` with validation and config editing support.
- Fetch recent-main PR metadata for the default branch and persist `recent-main.json`.
- Extend `plan.json` and `context.json` with mode-aware recent-main and sequential-step metadata.
- Write sequential `state.json` plus `steps/<NN>-pr-<NUMBER>/` artifacts.
- Publish one stacked step PR per sequential step and persist the base-branch chain.
- Resume sequential runs from the first incomplete step using persisted validation commands.
- Render checkpoint mode with deterministic `--render-once --events ...` proofs.

## Validation Scope

- `cargo test --test config -- --test-threads=1`
- `cargo test --test merge -- --test-threads=1`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `make quality`
- `cargo run -- merge --help`
- `cargo run -- runtime config --help`

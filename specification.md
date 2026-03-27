# Specification: Technical: Add sequential `meta merge` mode with GitHub-main-aware planning, per-step publication, and optional checkpoints

Version: 0.2  
Last updated: 2026-03-27

## 1. Executive Summary

Extend `meta merge` with an explicit sequential mode that applies and publishes selected pull requests one step at a time while preserving the current aggregate workflow as the default. Sequential mode must persist resumable step state, include recent-main GitHub context in planning and conflict prompts, and optionally pause at checkpoints between steps.

## 2. Problem Statement

- `src/merge.rs` previously assumed one aggregate branch, one validation pass, and one aggregate publication result.
- Planner and conflict prompts had no view of recently merged work on remote `main`.
- Resume state could only express aggregate publication, not partially completed per-step publication.
- Existing merge UX, progress tracking, and GitHub helpers were already mature enough to extend rather than replace.

Non-goals:

- changing aggregate mode when `--sequential` is not supplied
- auto-merging sequential step PRs after publication
- adding a second merge command family outside `meta merge`

## 3. Functional Requirements

1. Aggregate mode remains the default behavior.
2. `meta merge` adds `--sequential` and sequential-only `--checkpoints`.
3. `--checkpoints` requires `--sequential` and conflicts with `--json` and `--no-interactive`.
4. Install-scoped merge config adds `[merge].recent_main_limit` with default `10` and bounds `1..=50`.
5. Aggregate and sequential planning both fetch and persist bounded recent-main metadata in `.metastack/merge-runs/<RUN_ID>/recent-main.json`.
6. Planner prompts, stored plans, and conflict prompts include recent-main context.
7. Sequential plans include one ordered step per selected PR plus rationale and risk notes.
8. Sequential execution writes `state.json` plus per-step artifacts under `.metastack/merge-runs/<RUN_ID>/steps/<NN>-pr-<NUMBER>/`.
9. Sequential publication is stacked: step 1 targets the default branch and each later step targets the previous successful step branch.
10. `--resume-run` detects persisted mode from `context.json` and resumes aggregate or sequential runs without replaying completed sequential publication work.

## 4. Non-Functional Requirements

- Reliability: write run and step artifacts incrementally so interrupted sequential runs remain resumable.
- Safety: keep all merge, validation, and publication work inside the managed sibling workspace.
- Observability: surface mode, recent-main inputs, active step, and publication chain from artifacts and terminal output alone.
- Boundedness: recent-main discovery and stored prompt context remain capped by `recent_main_limit`.

## 5. Contracts and Interfaces

Inputs:

- `meta merge` flags, including `--sequential`, `--checkpoints`, `--resume-run`, `--validate`, and `--render-once`
- install-scoped `[merge]` config, including `recent_main_limit`
- GitHub repository, open-PR, and recent-main metadata through `src/github_pr.rs`

Run-level outputs:

- `context.json`
- `recent-main.json`
- `plan.json`
- `progress.json`
- `merge-progress.json`
- `publication.json`
- `state.json` for sequential runs

Sequential step outputs:

- `steps/<NN>-pr-<NUMBER>/step-context.json`
- `steps/<NN>-pr-<NUMBER>/step-progress.json`
- `steps/<NN>-pr-<NUMBER>/validation.json`
- `steps/<NN>-pr-<NUMBER>/publication.json`
- `steps/<NN>-pr-<NUMBER>/step-pr-body.md`
- optional conflict and validation-repair prompt/output artifacts

Compatibility:

- aggregate publication still writes one aggregate `publication.json`
- additive fields in `context.json` and `plan.json` remain backward-compatible with aggregate runs
- old aggregate resumes continue to work

## 6. Architecture and Data Flow

1. Resolve repository metadata and open PRs.
2. Resolve merge mode and merge config.
3. Fetch recent-main metadata and persist `recent-main.json`.
4. Build a mode-aware planning prompt.
5. Validate the returned plan.
6. Aggregate mode continues on one branch and one publication result.
7. Sequential mode initializes `state.json`, applies one PR per step, validates the cumulative stacked branch, and publishes one step PR per successful step.
8. Checkpoint mode pauses between sequential steps using the merge dashboard render-once/event model.
9. Resume reloads `context.json`, `plan.json`, `recent-main.json`, and `state.json` and continues from the first incomplete step.

## 7. Acceptance Criteria

- [x] Aggregate mode remains the default.
- [x] `--sequential` applies selected PRs one step at a time and records per-step outcomes.
- [x] `--checkpoints` provides an interactive sequential review surface with deterministic render proof coverage.
- [x] Recent-main context is fetched, persisted, and included in planner and conflict prompts.
- [x] Sequential runs write durable run-level and step-level artifacts and resume without replaying completed steps.
- [x] Documentation covers aggregate vs sequential behavior, checkpoint mode, and the stacked publication contract.

## 8. Test Plan

- `tests/config.rs` covers `recent_main_limit` persistence and validation.
- `tests/merge.rs` covers:
  - aggregate compatibility
  - sequential step artifacts
  - checkpoint render-once output
  - sequential resume from first incomplete step
  - aggregate publication retry regression coverage
- `cargo clippy --all-targets --all-features -- -D warnings`
- `make quality`

## 9. Open Questions

1. If an operator merges an earlier sequential step PR while a run is paused, should later resume keep the stored stacked base branch or rebase to refreshed `main`?
2. Should sequential publication ever retry automatically after the first create failure, or should resume remain the only retry path?

## 10. Linked Files

- `src/cli.rs`
- `src/config.rs`
- `src/config_command.rs`
- `src/github_pr.rs`
- `src/merge.rs`
- `src/merge_dashboard.rs`
- `tests/config.rs`
- `tests/merge.rs`
- `docs/merge.md`

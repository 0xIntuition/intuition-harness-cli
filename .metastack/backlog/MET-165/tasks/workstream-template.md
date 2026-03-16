# Workstream: merge-command-and-runner-foundation

Last updated: 2026-03-16

Parent index: [`../index.md`](../index.md)
Parent specification: [`../specification.md`](../specification.md)

## Objective

Ship the first end-to-end `meta merge` slice that can discover PRs, demonstrate the selection flow deterministically, create a safe workspace, persist run artifacts, and drive aggregate publication through hermetic tests.

## Scope

In scope:
- add the top-level `meta merge` command surface and help text
- implement `gh`-backed repo and PR discovery with deterministic `--json`
- implement the one-shot dashboard and `--render-once` snapshot path
- add merge-run workspace creation and artifact persistence under `.metastack/merge-runs/<RUN_ID>/`
- invoke the configured local agent for planning and conflict-resolution paths
- rerun validation and gate aggregate PR publication on success

Out of scope:
- background merge daemons or listener-style persistent sessions
- cross-repository batching or non-`main` base targets in the first slice
- replacing GitHub's hosted merge queue or approval model
- unrelated cleanup of existing dashboard or listener modules

## Files and Areas

- `src/cli.rs`
- `src/lib.rs`
- `src/fs.rs`
- `src/listen/workspace.rs`
- `src/agents/execution.rs`
- `src/merge.rs` or `src/merge/*`
- `README.md`
- `WORKFLOW.md`
- `prompts/injected-agent-workflow-contract.md`
- `tests/merge.rs`
- `tests/commands.rs`
- `tests/support/common.rs`

## Implementation Tasks

- [ ] Define args, help text, and dispatch for `meta merge`.
- [ ] Add the `gh` adapter for repo resolution, PR discovery, and aggregate PR upsert.
- [ ] Define and test the `--json` output shape consumed by the dashboard and planner.
- [ ] Build dashboard state, selection summary, cancel behavior, and scripted snapshot coverage.
- [ ] Extend path handling for `.metastack/merge-runs/<RUN_ID>/` and write the initial run manifest.
- [ ] Build the merge-planning prompt with repo scope, selected PRs, and conflict hotspots.
- [ ] Apply selected PRs in explicit order and record per-PR progress.
- [ ] Invoke the agent again on conflict and persist a blocker report when resolution fails.
- [ ] Run validation commands and refuse publication on failure.
- [ ] Publish or update the aggregate PR and document the operator workflow.

## Test Plan

- Unit:
  - `gh` JSON parsing and error mapping
  - branch-name or run-id formatting helpers
  - artifact manifest rendering and PR body formatting
- Integration:
  - stub `gh` on `PATH`
  - temp repos with synthetic PR branches
  - ratatui snapshot tests with scripted events
- Failure modes:
  - missing auth or missing `gh`
  - repo-resolution failure
  - empty PR set or empty selection
  - conflict unresolved by the agent
  - validation failure after local merge
  - aggregate PR publication failure

## Handoff Notes

- Current status: planned
- Next unblocker: lock the artifact schema and non-interactive execution contract for end-to-end tests
- Reviewer callouts: workspace safety and hermetic GitHub coverage are the highest-risk areas

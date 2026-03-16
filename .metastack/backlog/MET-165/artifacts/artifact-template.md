# Artifact: merge-run-contract-proof

Last updated: 2026-03-16

## Purpose

Capture the first implementation-backed proof for the `.metastack/merge-runs/<RUN_ID>/` contract, including the expected files, the command path that produced them, and the aggregate PR publication summary.

## Inputs

- Parent issue `MET-161` acceptance criteria
- `src/fs.rs` path conventions
- `src/listen/workspace.rs` workspace safety rules
- `src/agents/execution.rs` subprocess-based agent integration
- Existing dashboard snapshot patterns from `src/setup.rs`, `src/sync_dashboard.rs`, and `src/linear/dashboard.rs`

## Output

A concise artifact describing the run directory manifest, the proof commands used to populate it, and the observed success or blocker state for clean and conflict-driven batches.

## Linked Decisions

- `D-002`
- `D-003`
- `D-004`

## Follow-ups

- [ ] Replace this seed artifact with a real run-layout proof after the first green temp-repo integration test.
- [ ] Add a snapshot of the aggregate PR body and selected PR summary once publication flow is implemented.

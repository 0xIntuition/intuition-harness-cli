# Workstream: readiness-evaluation-and-batch-kickoff

Last updated: 2026-03-16

Parent index: [`../index.md`](../index.md)
Parent specification: [`../specification.md`](../specification.md)

## Objective

Add a shared readiness evaluator for Todo issues, expose it through a read-only command, and integrate the resulting `ready` subset into bounded `meta agents listen` pickup without breaking current workspace, session, or review flows.

## Scope

In scope:

- readiness classification, score calculation, and explanation text
- listen config and CLI plumbing for bounded concurrency
- reuse of the existing listen session store and dashboard surfaces
- targeted Linear data-shape changes needed for dependency filtering
- tests and docs for the new behavior

Out of scope:

- full listen lifecycle modularization
- redesigning the backlog/workpad contract
- changing the source-repo vs workspace execution rule
- introducing a new persistence backend

## Files and Areas

- `src/cli.rs`
- `src/lib.rs`
- `src/config.rs`
- `src/setup.rs`
- `src/listen/mod.rs`
- `src/listen/state.rs`
- `src/listen/store.rs`
- `src/listen/dashboard.rs`
- `src/listen/web.rs`
- `src/listen/workspace.rs`
- `src/linear/types.rs`
- `src/linear/service.rs`
- `src/linear/transport.rs`
- `tests/cli.rs`
- `README.md`
- `docs/agent-daemon.md`

## Implementation Tasks

- [ ] Define the readiness result type, score bands, and reason taxonomy.
- [ ] Add the read-only readiness CLI command and any needed JSON output mode.
- [ ] Add the new listen concurrency setting to config and setup flows.
- [ ] Make listen evaluate the queue before pickup and enforce the active-session cap.
- [ ] Persist or summarize readiness-driven outcomes through the existing session store.
- [ ] Update queue, inspect, and dashboard wording so operators can see why issues were skipped.
- [ ] Add regression tests for dependency filtering, missing context, concurrency, and workspace safety.
- [ ] Update docs and help text for the new command path and defaults.

## Test Plan

- Unit:
  - readiness classification helpers
  - session counting and concurrency gating
- Integration:
  - readiness command output
  - one-cycle bounded listen pickup with multiple Todo issues
  - session inspect output after pickup
- Failure modes:
  - blocked dependencies
  - missing description or validation context
  - workspace conflict or unsafe reuse path
  - legacy state files without readiness metadata

## Handoff Notes

- Current status: planned
- Next unblocker: confirm the minimum dependency signal needed from Linear to satisfy the acceptance bar
- Reviewer callouts: check naming clarity between `max_pickups` and the new concurrency limit, and confirm default behavior stays safe for current users

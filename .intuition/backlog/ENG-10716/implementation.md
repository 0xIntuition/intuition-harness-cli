# Implementation Plan

Last updated: 2026-04-06

## Workstreams

1. Add the install-scoped listen retry contract and shared Linear failure classifier.
2. Keep daemon polling and reconciliation alive during transient Linear outages while preserving
   the last known queue and session snapshot.
3. Persist deferred worker-side Linear sync and replay it across `listen`, `execute`, and
   `resume` once Linear recovers.
4. Align operator docs and backlog evidence with the delivered behavior.

## Touchpoints

- Runtime config and CLI surface:
  - [`../../../src/cli.rs`](../../../src/cli.rs)
  - [`../../../src/config.rs`](../../../src/config.rs)
  - [`../../../src/config_resolution.rs`](../../../src/config_resolution.rs)
  - [`../../../src/config_command.rs`](../../../src/config_command.rs)
  - [`../../../tests/config.rs`](../../../tests/config.rs)
- Shared Linear failure handling:
  - [`../../../src/linear/service.rs`](../../../src/linear/service.rs)
  - [`../../../src/linear/mod.rs`](../../../src/linear/mod.rs)
  - [`../../../src/linear/service/tests.rs`](../../../src/linear/service/tests.rs)
- Listen daemon, dashboard, and persisted state:
  - [`../../../src/listen/mod.rs`](../../../src/listen/mod.rs)
  - [`../../../src/listen/preflight.rs`](../../../src/listen/preflight.rs)
  - [`../../../src/listen/dashboard.rs`](../../../src/listen/dashboard.rs)
  - [`../../../src/listen/state.rs`](../../../src/listen/state.rs)
  - [`../../../src/listen/store.rs`](../../../src/listen/store.rs)
  - [`../../../tests/listen.rs`](../../../tests/listen.rs)
- Worker-side deferred sync replay:
  - [`../../../src/listen/worker.rs`](../../../src/listen/worker.rs)
  - [`../../../tests/listen.rs`](../../../tests/listen.rs)
- Docs and workflow guidance:
  - [`../../../README.md`](../../../README.md)
  - [`../../../docs/agent-daemon.md`](../../../docs/agent-daemon.md)
  - [`../../../docs/listen-phased-execution-spec.md`](../../../docs/listen-phased-execution-spec.md)
  - [`../../../WORKFLOW.md`](../../../WORKFLOW.md)
  - [`./specification.md`](./specification.md)
  - [`./validation.md`](./validation.md)

## Delivered Contract Mapping

### Install-scoped Retry Contract

- `src/config.rs` adds `InstallListenSettings.retry`, `ListenRetrySettings`, built-in defaults,
  and shared backoff calculation.
- `src/config_resolution.rs` validates the retry bounds and the `max >= initial` invariant.
- `src/cli.rs` exposes `--listen-retry-initial-backoff` and `--listen-retry-max-backoff`.
- `src/config_command.rs` renders the effective retry contract in text and JSON output.
- `tests/config.rs` proves config persistence, effective rendering, and validation failures.

### Shared Failure Classification

- `src/linear/service.rs` owns `LinearFailureKind`, `LinearFailure`, and the shared classifier.
- `src/linear/mod.rs` re-exports the classifier for listen-family callers.
- `src/listen/preflight.rs`, `src/listen/mod.rs`, and `src/listen/worker.rs` consume the shared
  classifier instead of local ad hoc string matching.

### Daemon Degraded-State Behavior

- `src/listen/mod.rs` resolves viewer scope per cycle, tolerates retryable preflight failures for
  non-`--check` runs, and builds degraded cycle data instead of exiting on transient Linear
  outages.
- `src/listen/state.rs` introduces `LinearFailureSnapshot` so degraded state can carry failure
  kind, message, status code, streak count, and next retry time.
- `src/listen/store.rs` persists that degraded state in the existing install-scoped listen store
  without requiring a breaking schema change.
- `src/listen/dashboard.rs` and textual inspection surfaces render the degraded state without
  hiding existing sessions or queue data.

### Worker Deferred-Sync Replay

- `src/listen/state.rs` introduces `PendingLinearSync` for issue refresh, workpad sync, PR
  attachment, and review-transition replay.
- `src/listen/worker.rs` persists pending sync when post-progress Linear operations fail and now
  retries that persisted sync immediately on the next worker invocation instead of waiting for the
  previous backoff window.
- `src/listen/mod.rs` preserves degraded queue/session state when only session snapshots change, so
  later worker and inspect flows continue to see the last known daemon snapshot.
- `tests/listen.rs` proves replay survives a transient review-transition failure and clears the
  pending state after recovery.

## Sequencing Notes

- The retry contract and classifier landed first so daemon and worker recovery paths reuse one
  contract instead of growing more private retry logic.
- Daemon degradation was implemented before worker replay so the last known state remained visible
  while deferred sync behavior was still in progress.
- Store changes remain backward-compatible by keeping all new degraded and pending-sync fields
  optional.

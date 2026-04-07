# Specification: Technical: Harden shared listen session flows against Linear failures and preserve resumability

Version: 0.1  
Last updated: 2026-04-06

Parent index: [`./index.md`](./index.md)

## 1. Executive Summary

Add one shared Linear failure contract plus install-scoped listen retry defaults so unattended
`meta agents listen` runs preserve queue visibility during transient Linear outages, and
worker-side post-progress Linear sync can resume after recovery without discarding local workspace
state.

## 2. Problem Statement

- Problem: listen-related flows still split Linear failure handling across private string matching,
  daemon hard-fail exits, and worker-side remote mutations that can block or terminate sessions
  after backlog, workspace, and agent history already exist.
- Why now: `ENG-10713` requires one coherent resiliency contract across install-scoped config,
  daemon polling and reconciliation, and worker-side recovery for `listen`, `execute`, and
  `resume`.
- Non-goals:
  - redesign unrelated Linear commands outside listen-family code paths
  - change repo-scoped pickup filters such as `required_labels`, `assignment_scope`, or workspace
    refresh policy semantics
  - replace the existing install-scoped listen store with a new persistence root

## 3. Functional Requirements

1. Install-scoped listen retry settings must be persisted through `crate::config::AppConfig`,
   surfaced by `meta runtime config`, and resolved through one shared default contract.
2. A shared `LinearFailureKind` classifier must distinguish transient, authentication,
   permission, configuration, and other Linear failures from HTTP status, GraphQL payload text,
   and transport failures.
3. Listen preflight, daemon polling, reconciliation, and worker-side issue refresh must consume
   the shared classifier instead of maintaining private retry or transient matchers.
4. Transient Linear failures during viewer lookup, issue listing, or reconciliation must degrade
   the daemon instead of terminating it, while preserving the last known queue and session
   snapshot.
5. Summary, dashboard, `--once --json`, and `meta listen sessions inspect` output must surface
   degraded-state evidence including failure kind, failure message, and retry timing.
6. Authentication, permission, and configuration failures must remain clearly operator-actionable
   instead of being treated as retryable transient outages.
7. After local workspace progress exists, later Linear failures in issue refresh, workpad sync,
   PR attachment, or review-state transitions must persist deferred sync state instead of forcing
   a clean restart.
8. The next `meta agents listen`, `meta agents execute`, or `meta listen sessions resume`
   invocation must replay pending sync state after Linear recovers and clear that persisted state
   on success.
9. Existing `session.json` and `session-details/*.json` artifacts without degraded-state or
   pending-sync fields must continue to deserialize cleanly.
10. README and affected workflow docs must describe the retry settings, default behavior, and the
    inherited listen-family recovery flows.

## 4. Non-Functional Requirements

- Performance: retry scheduling must stay bounded and avoid tight failure loops during outages.
- Reliability: identical failure sequences must produce the same classification, retry timing, and
  persisted degraded or pending-sync state.
- Security: no new secrets are persisted in repo-local artifacts; listen retry behavior continues
  to use install-scoped config and existing Linear auth resolution.
- Observability: operators must be able to inspect the latest Linear failure, retry timing, and
  deferred sync work from CLI output or persisted session artifacts.

## 5. Contracts and Interfaces

### 5.1 Inputs

- Input shape:
  - install-scoped retry settings under `[defaults.listen.retry]`
  - `meta runtime config` flags `--listen-retry-initial-backoff` and
    `--listen-retry-max-backoff`
  - Linear HTTP, GraphQL, and network errors emitted through the shared transport and service
    layers
  - persisted listen session state under the install-scoped `listen/projects/<PROJECT_KEY>/`
    store
- Validation rules:
  - retry backoff values must be within `1..=3600`
  - max backoff must be greater than or equal to initial backoff
  - unset config values fall back to built-in defaults of `2s` initial and `60s` max
  - degraded and pending-sync fields remain optional for backward-compatible deserialization

### 5.2 Outputs

- Output shape:
  - effective retry settings rendered by `meta runtime config` text and JSON output
  - `LinearFailureKind` plus optional HTTP status for shared failure classification
  - degraded Linear snapshot state with failure message, observed time, failure streak, and next
    retry time
  - pending Linear sync state for replayable worker operations
- Error shape:
  - retryable transient failures remain visible as degraded state with backoff timing
  - non-retryable auth, permission, and configuration failures remain visible as operator-action
    items instead of being silently retried

### 5.3 Compatibility

- Backward-compat constraints:
  - existing session-state files without new fields must still load
  - repo-scoped `.metastack/meta.json` listen settings remain unchanged in this slice
  - existing workspace-safety guarantees and PR handoff behavior remain in force
- Migration plan:
  - add only optional store fields
  - keep existing store versioning and log compatibility behavior intact
  - rely on the next successful session write to persist the new fields

## 6. Architecture and Data Flow

- High-level flow:
  - install-scoped config resolves the shared retry policy
  - Linear transport and service layers classify failures once
  - daemon polling persists degraded-state metadata instead of exiting on transient failures
  - worker-side publication and reconciliation persist deferred sync work for replay
- Key components:
  - config and CLI: [`../../../src/cli.rs`](../../../src/cli.rs),
    [`../../../src/config.rs`](../../../src/config.rs),
    [`../../../src/config_resolution.rs`](../../../src/config_resolution.rs),
    [`../../../src/config_command.rs`](../../../src/config_command.rs)
  - Linear failure classification: [`../../../src/linear/service.rs`](../../../src/linear/service.rs)
  - daemon and persisted listen state: [`../../../src/listen/mod.rs`](../../../src/listen/mod.rs),
    [`../../../src/listen/state.rs`](../../../src/listen/state.rs),
    [`../../../src/listen/store.rs`](../../../src/listen/store.rs),
    [`../../../src/listen/dashboard.rs`](../../../src/listen/dashboard.rs)
  - worker deferred sync replay: [`../../../src/listen/worker.rs`](../../../src/listen/worker.rs)
- Boundaries:
  - install-scoped retry config governs listen-family Linear recovery behavior
  - repo-scoped validation and pickup filters continue to live in `.metastack/meta.json`
  - persisted listen state remains install-scoped while code and backlog artifacts stay repo-local

## 7. Acceptance Criteria

- Install-scoped listen retry defaults can be viewed and updated through `meta runtime config`
  and validate cleanly.
- A shared `LinearFailureKind` replaces listen-specific transient matching and distinguishes
  retryable from operator-actionable Linear failures.
- `meta agents listen` preserves queue and session visibility during transient viewer, listing, or
  reconciliation failures instead of exiting.
- Summary, dashboard, and inspect output surface degraded Linear state and retry timing.
- Worker-side Linear sync remains resumable after local progress exists and clears pending sync
  state after recovery.
- README and workflow docs describe the retry contract, degraded-state behavior, and inherited
  `listen` / `execute` / `resume` replay behavior.

## 8. Test Plan

- Unit tests:
  - shared failure classification coverage in
    [`../../../src/linear/service/tests.rs`](../../../src/linear/service/tests.rs)
- Integration tests:
  - runtime config JSON and validation coverage in [`../../../tests/config.rs`](../../../tests/config.rs)
  - degraded daemon and worker replay coverage in [`../../../tests/listen.rs`](../../../tests/listen.rs)
- Contract tests:
  - `meta runtime config --json` effective retry shape validated in integration tests
  - persisted session-state compatibility exercised through listen-state serialization and reloads
- Negative-path tests:
  - mocked `429`, `503`, and network-level daemon failures
  - retry-boundary validation for listen retry config
  - deferred worker review-transition replay after a transient Linear failure and later recovery

## 9. Open Questions

1. Worker-specific retry caps are still a future follow-up; this slice uses one shared listen
   retry contract for both daemon degradation and worker-side pending-sync replay timing.
2. Compact dashboard rendering currently surfaces degraded state at the session/runtime level; any
   richer per-row failure rendering should be a separate UX follow-up if signal proves insufficient.

## 10. Linked Workstreams

- Shared implementation workstream: [`./tasks/workstream-template.md`](./tasks/workstream-template.md)

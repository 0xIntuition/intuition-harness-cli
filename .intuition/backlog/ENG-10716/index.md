# Technical: Harden shared listen session flows against Linear failures and preserve resumability

## Parent Issue

* Identifier: `ENG-10713`
* Title: Harden shared listen session flows against Linear failures and preserve resumability
* State: `Staging`
* Project: `CLI Harness`
* URL: [ENG-10713](https://linear.app/0xintuition/issue/ENG-10713/harden-shared-listen-session-flows-against-linear-failures-and)

## Context

`metastack-cli` already has the durable state needed for resilient listen execution, but its Linear-failure handling is fragmented. Install-scoped defaults in [`../../../src/config.rs`](<../../../src/config.rs>) and [`../../../src/config_command.rs`](<../../../src/config_command.rs>) cover poll interval, refresh policy, and merge retries, yet nothing defines a shared listen retry/backoff contract. The runtime instead duplicates a narrow transient matcher across [`../../../src/listen/mod.rs`](<../../../src/listen/mod.rs>), [`../../../src/listen/preflight.rs`](<../../../src/listen/preflight.rs>), and [`../../../src/listen/worker.rs`](<../../../src/listen/worker.rs>).

That fragmentation leaks into runtime behavior. [`../../../src/listen/mod.rs`](<../../../src/listen/mod.rs>) exits the live loop when `run_cycle()` returns an error, so a temporary Linear outage can still kill the daemon. [`../../../src/listen/mod.rs`](<../../../src/listen/mod.rs>) and [`../../../src/listen/worker.rs`](<../../../src/listen/worker.rs>) also fail hard on recoverable workpad, PR-attachment, or review-transition operations even after the workspace, backlog packet, and local agent history already exist. At the same time, [`../../../src/listen/store.rs`](<../../../src/listen/store.rs>) and [`../../../src/listen/state.rs`](<../../../src/listen/state.rs>) already persist enough context to make those failures resumable: workpad IDs, workspace paths, backlog refs, PR metadata, milestones, and provider-native resume handles.

## Proposed Approach

1. Add an install-scoped listen retry policy plus a shared `LinearFailureKind` classifier. Wire it through [`../../../src/cli.rs`](<../../../src/cli.rs>), [`../../../src/config.rs`](<../../../src/config.rs>), [`../../../src/config_resolution.rs`](<../../../src/config_resolution.rs>), and [`../../../src/config_command.rs`](<../../../src/config_command.rs>), then consume the classifier from the listen preflight, daemon, and worker paths instead of duplicating string matching.
2. Teach the daemon to degrade instead of exit. Update [`../../../src/listen/mod.rs`](<../../../src/listen/mod.rs>), [`../../../src/listen/dashboard.rs`](<../../../src/listen/dashboard.rs>), [`../../../src/listen/state.rs`](<../../../src/listen/state.rs>), and [`../../../src/listen/store.rs`](<../../../src/listen/store.rs>) so transient Linear failures during issue listing, viewer refresh, or reconciliation preserve existing session visibility and surface last-failure plus backoff timing in summary, dashboard, and inspect output.
3. Make worker-side Linear sync resumable. Update [`../../../src/listen/worker.rs`](<../../../src/listen/worker.rs>), [`../../../src/listen/mod.rs`](<../../../src/listen/mod.rs>), [`../../../src/linear/service/workpad.rs`](<../../../src/linear/service/workpad.rs>), [`../../../src/linear/service/assets.rs`](<../../../src/linear/service/assets.rs>), and [`../../../src/github_pr.rs`](<../../../src/github_pr.rs>) so post-progress failures are deferred into persisted pending-sync state and replayed successfully across `meta agents execute`, `meta agents listen`, and `meta listen sessions resume` when Linear recovers.

## Risks

* A loose failure classifier could retry auth or configuration errors forever and hide real operator action items.
* Persisting deferred sync state without strong idempotency could duplicate workpad comments, PR attachments, or review-state transitions after recovery.
* Degraded-state UI could crowd out the existing session list if failure details are rendered too prominently in compact views.
* Session-store schema changes must remain backward compatible with already persisted `session.json` and `session-details/*.json` artifacts.

## Validation

- [x] `cargo test --test config`
- [x] `cargo test --test listen`
- [x] `cargo test linear::service`
- [x] `cargo test linear::transport`
- [x] `cargo clippy --all-targets --all-features -- -D warnings`
- [x] `make quality`
- [x] Deterministic proof that mocked `429`, `503`, and network-level Linear failures preserve existing session visibility and surface degraded-state or backoff evidence instead of terminating the daemon
- [x] Deterministic proof that a session with local workspace progress survives a later transient Linear failure, resumes successfully, and clears pending sync state once Linear is reachable again

## Definition of Done

- [x] Scope and non-goals are captured in [`./specification.md`](<./specification.md>).
- [x] The retry or config contract and shared failure-classification contract are mapped to concrete repo paths in [`./implementation.md`](<./implementation.md>).
- [x] Daemon degraded-state behavior and worker deferred-sync behavior are both covered by tests or deterministic command proofs.
- [x] README and affected workflow docs explain the new listen retry settings, default behavior, and inherited flows.
- [x] Validation evidence is recorded in [`./validation.md`](<./validation.md>).

<!-- metastack-listen-progress:start -->
## Listener Progress Checklist

### Completed

- [x] Changed `README.md`
- [x] Changed `WORKFLOW.md`
- [x] Changed `docs/agent-daemon.md`
- [x] Changed `docs/listen-phased-execution-spec.md`
- [x] Changed `src/cli.rs`
- [x] Changed `src/config.rs`
- [x] Changed `src/config_command.rs`
- [x] Changed `src/config_resolution.rs`
- [x] Changed `src/linear/mod.rs`
- [x] Changed `src/linear/service.rs`
- [x] Changed `src/linear/service/tests.rs`
- [x] Changed `src/listen/dashboard.rs`
- [x] Changed `src/listen/mod.rs`
- [x] Changed `src/listen/preflight.rs`
- [x] Changed `src/listen/state.rs`
- [x] Changed `src/listen/store.rs`
- [x] Changed `src/listen/worker.rs`
- [x] Changed `tests/config.rs`
- [x] Changed `tests/listen.rs`
- [x] Changed `tests/support/common.rs`
- [x] Changed `.intuition/backlog/ENG-10716/checklist.md`
- [x] Changed `.intuition/backlog/ENG-10716/implementation.md`
- [x] Changed `.intuition/backlog/ENG-10716/index.md`
- [x] Changed `.intuition/backlog/ENG-10716/specification.md`
- [x] Changed `.intuition/backlog/ENG-10716/validation.md`

### Remaining

- [ ] Performance budget validated.

### Validation

- [x] `cargo test --test config`
- [x] `cargo test --test listen`
- [x] `cargo test linear::service`
- [x] `cargo test linear::transport`
- [x] `cargo clippy --all-targets --all-features -- -D warnings`
- [x] `make quality`
- [x] Deterministic proof that mocked `429`, `503`, and network-level Linear failures preserve existing session visibility and surface degraded-state or backoff evidence instead of terminating the daemon
- [x] Deterministic proof that a session with local workspace progress survives a later transient Linear failure, resumes successfully, and clears pending sync state once Linear is reachable again
<!-- metastack-listen-progress:end -->

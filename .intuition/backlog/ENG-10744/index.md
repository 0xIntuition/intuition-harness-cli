# Technical: Detect, classify, and recover stale listen workers with bounded retries

## Parent Issue

* Identifier: `ENG-10743`
* Title: Detect, classify, and recover stale listen workers with bounded retries
* State: `Staging`
* Team: `ENG`
* URL: [ENG-10743](https://linear.app/0xintuition/issue/ENG-10743/detect-classify-and-recover-stale-listen-workers-with-bounded-retries)

## Context

`metastack-cli` already keeps an install-scoped listen store, a mirrored `session-details/<ISSUE>.json` detail artifact, and a reconciliation loop that can relaunch workers from saved workspace and workpad context. The current stale-worker path stops short of using that capability.

Repo evidence:

* [`../../../src/listen/mod.rs`](<../../../src/listen/mod.rs>) now routes dead worker detection through centralized stale-worker classification and bounded retry logic.
* [`../../../src/listen/store.rs`](<../../../src/listen/store.rs>) rewrites malformed detail artifacts, mirrors session fields into detail JSON, and exposes the existing manual `retry_blocked_session(...)` affordance.
* [`../../../src/listen/worker.rs`](<../../../src/listen/worker.rs>) rewrites the active session repeatedly throughout execution, so recovery metadata must survive ongoing worker session writes.
* [`../../../src/listen/dashboard.rs`](<../../../src/listen/dashboard.rs>) and `meta listen sessions inspect` carry the stale-worker recovery detail without adding a new pane.

## Proposed Approach

1. Add additive recovery fields to the persisted listen session contract: `started_at_epoch_seconds`, `stale_worker_recovery_attempt_count`, and `latest_stale_worker_failure`.
2. Replace the stale-PID path in reconciliation with one classify-then-recover helper that uses the `ENG-10736` blocked taxonomy to decide retryable versus terminal stale-worker outcomes.
3. Auto-restart only listen-origin `running` sessions that still have the required workspace and workpad context, using the existing `spawn_listen_worker_from_context(...)` path so backlog linkage, workspace path, and workpad comment id are preserved.
4. Cap automatic stale-worker recovery at `2` attempts per operator-started run. When the cap is exhausted or classification says non-retryable, keep the session blocked with a structured terminal reason and leave the existing manual retry path intact.
5. Extend inspect and selected-session detail rendering with started time, elapsed time since original start, recovery attempt count, and latest stale-worker failure.

## Risks

* A recovery loop could accidentally spawn duplicate workers if dead-PID detection and session rewrite ordering are not centralized.
* Old state files could lose data or show the wrong elapsed time if `started_at_epoch_seconds` backfill is not deterministic.
* Recovery metadata could drift between `session.json`, `session-details/<ISSUE>.json`, inspect output, and dashboard detail if all surfaces do not read from one mirrored source.

## Validation

- [x] `cargo test reconcile_sessions_blocks_paused_session_with_dead_worker_pid --lib`
- [x] `cargo test stale_running_session_relaunches_with_existing_context_and_fresh_pid --lib`
- [x] `cargo test stale_running_session_budget_exhaustion_blocks_without_relaunch --lib`
- [x] `cargo test stale_worker_recovery_persists_metadata_before_replacement_worker_write --lib`
- [x] `cargo test --test listen -- --test-threads=1`
- [x] `cargo test listen_surfaces_no_checks_configured_in_inspect_and_dashboard --test listen -- --exact`
- [x] `cargo test listen::store --lib`
- [x] `cargo test listen::dashboard --lib`
- [x] `cargo test listen::mod --lib`
- [x] `cargo run -- agents review --check --root .`
- [x] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test 'managed_child::tests::timeout_callback_failure_still_reaps_captured_process_group' --lib -- --exact`
- [ ] `cargo test 'managed_child::tests::timeout_kills_process_group_and_reaps_children' --lib -- --exact`
- [ ] `make quality`
- [x] Focused compatibility proof that old `session.json` and `session-details/<ISSUE>.json` payloads load and rewrite with the new fields while preserving existing data
- [x] Focused recovery proof that a dead listen-origin `running` worker is relaunched with the same workspace path, workpad comment id, backlog linkage, and a fresh PID
- [x] Focused exhaustion proof that retry-budget exhaustion parks the session as blocked with a structured reason and does not auto-relaunch again
- [x] Focused paused-session proof that a dead stored PID on a paused session is classified and parked as blocked
- [x] Focused inspect and dashboard proof that recovery count, latest stale failure, and elapsed-since-start render in operator output

## Definition of Done

- [x] Parent scope, non-goals, and landing dependencies are captured in this packet.
- [x] [`./specification.md`](<./specification.md>) names the concrete session fields, recovery budget, classification rules, and operator-surface requirements.
- [x] [`./implementation.md`](<./implementation.md>) maps the work to real repo paths and explains why no new config surface is introduced.
- [x] [`./validation.md`](<./validation.md>) records deterministic command proofs for compatibility, auto-recovery, exhaustion, and rendering.

<!-- metastack-listen-progress:start -->
## Listener Progress Checklist

### Completed

- [x] Changed `artifacts/validation/ENG-10744.md`
- [x] Changed `tests/listen.rs`
- [x] Changed `.intuition/backlog/ENG-10744/index.md`
- [x] Changed `.intuition/backlog/ENG-10744/validation.md`

### Remaining

- [ ] Resolve the deterministic `managed_child` timeout-reaping test failures in the local gate.
- [ ] Rerun `make quality`.
- [ ] If the local gate turns green on a follow-up pass, rerun exact-SHA GitHub verification for the published head.
- [ ] Refresh remote-only PR/workpad metadata if a metadata-capable automation path becomes available.

### Validation

- [x] `cargo test --test listen -- --test-threads=1`
- [x] `cargo test listen_surfaces_no_checks_configured_in_inspect_and_dashboard --test listen -- --exact`
- [x] `cargo test listen::store --lib`
- [x] `cargo test listen::dashboard --lib`
- [x] `cargo test listen::mod --lib`
- [x] `cargo run -- agents review --check --root .`
- [x] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test 'managed_child::tests::timeout_callback_failure_still_reaps_captured_process_group' --lib -- --exact`
- [ ] `cargo test 'managed_child::tests::timeout_kills_process_group_and_reaps_children' --lib -- --exact`
- [ ] Local validation profile `heuristic` must pass: `make quality`
<!-- metastack-listen-progress:end -->

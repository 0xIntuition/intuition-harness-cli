# Validation Plan

Last updated: 2026-04-06

## Command Proofs

- [x] `cargo test --test config`
- [x] `cargo test --test listen`
- [x] `cargo test linear::service`
- [x] `cargo test linear::transport`
- [x] `cargo clippy --all-targets --all-features -- -D warnings`
- [x] `make quality`

## Deterministic Behavior Proofs

- [x] Runtime-config proof:
  `tests/config.rs::config_updates_listen_retry_defaults_and_renders_effective_values`
  verifies `meta runtime config --json` renders the effective listen retry policy and persists it
  under `[defaults.listen.retry]`.
- [x] Retry validation proof:
  `tests/config.rs::config_rejects_listen_retry_max_backoff_below_initial`
  verifies invalid retry bounds fail with a clear validation error.
- [x] Shared classifier proof:
  `src/linear/service/tests.rs::classify_linear_failure_distinguishes_retryable_and_operator_errors`
  covers transient, authentication, permission, configuration, and other failure kinds.
- [x] Daemon transient proof:
  `tests/listen.rs::listen_once_degraded_429_preserves_existing_session_visibility`,
  `tests/listen.rs::listen_once_degraded_503_preserves_existing_session_visibility`, and
  `tests/listen.rs::listen_once_degraded_network_failure_preserves_existing_session_visibility`
  prove mocked `429`, `503`, and network failures keep the last known session snapshot visible,
  persist degraded state, and surface degraded evidence in `--once --json` and
  `listen sessions inspect`.
- [x] Worker resumability proof:
  `tests/listen.rs::listen_worker_replays_pending_review_transition_after_linear_recovery`
  proves a later transient Linear failure after local workspace progress persists
  `pending_linear_sync`, and that a later successful run clears that state.

## Notes

- The validation set stayed deterministic and mocked; no live Linear mutation was required to prove
  the new retry, degraded-state, or replay contracts.
- The primary Linear issue description remained untouched; ticket progress and validation updates
  are recorded in the single `## Codex Workpad` comment instead of `backlog sync push`.

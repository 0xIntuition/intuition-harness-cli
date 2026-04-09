# Validation Plan

Last updated: 2026-04-09

## Command Proofs

- [x] `cargo test reconcile_sessions_blocks_paused_session_with_dead_worker_pid --lib`
- [x] `cargo test stale_running_session_relaunches_with_existing_context_and_fresh_pid --lib`
- [x] `cargo test stale_running_session_budget_exhaustion_blocks_without_relaunch --lib`
- [x] `cargo test stale_worker_recovery_persists_metadata_before_replacement_worker_write --lib`
- [x] `cargo test listen::store --lib`
- [x] `cargo test listen::dashboard --lib`
- [x] `cargo test listen::mod --lib`
- [x] `cargo test --test listen -- --test-threads=1`
- [x] `cargo test listen_surfaces_no_checks_configured_in_inspect_and_dashboard --test listen -- --exact`
- [x] `cargo run -- agents review --check --root .`
- [x] `cargo clippy --all-targets --all-features -- -D warnings`
- [x] `make quality`

## Deterministic Behavior Proofs

- [x] Paused-session stale-worker proof:
  `src/listen/mod.rs::tests::reconcile_sessions_blocks_paused_session_with_dead_worker_pid`
  verifies the outer reconciliation loop no longer retains a paused session whose stored PID is
  dead and instead blocks it with the structured reason `paused worker died`.
- [x] Auto-recovery proof:
  `src/listen/mod.rs::tests::stale_running_session_relaunches_with_existing_context_and_fresh_pid`
  verifies a dead listen-origin `running` worker relaunches with the same workspace path, workpad
  comment id, backlog linkage, and a fresh PID.
- [x] Pre-spawn persistence proof:
  `src/listen/mod.rs::tests::stale_worker_recovery_persists_metadata_before_replacement_worker_write`
  verifies reconciliation persists stale-worker metadata before replacement-worker launch so an
  early worker-side session rewrite cannot erase the recovery count, original start time, or
  latest stale-worker failure.
- [x] Exhaustion proof:
  `src/listen/mod.rs::tests::stale_running_session_budget_exhaustion_blocks_without_relaunch`
  verifies the `2`-attempt automatic recovery budget parks the session as blocked with a structured
  reason and does not relaunch again.
- [x] Compatibility proof:
  `src/listen/store.rs::tests::load_state_backfills_stale_worker_fields_from_old_payloads`
  verifies legacy `session.json` and `session-details/<ISSUE>.json` payloads load, backfill the
  new fields, and preserve the existing session summary, tokens, and timestamps.
- [x] Manual retry reset proof:
  `src/listen/store.rs::tests::retry_blocked_session_resets_stale_worker_recovery_window`
  verifies a blocked stale-worker session resets its start timestamp, clears the latest stale
  failure, and zeroes the automatic recovery count only when the operator explicitly retries it.
- [x] Inspect rendering proof:
  `tests/listen.rs::listen_sessions_inspect_renders_stale_worker_recovery_metadata`
  verifies `meta listen sessions inspect` renders recovery attempts, latest stale-worker failure,
  and elapsed since original start for both the session and mirrored detail artifact.
- [x] Dashboard rendering proof:
  `src/listen/dashboard.rs::tests::session_detail_renders_stale_worker_recovery_metadata`
  verifies selected-session detail text renders elapsed since start, recovery attempts, latest
  stale-worker failure, and latest stale-worker observation time.

## Notes

- The previous packet overstated paused-session coverage; this refresh records the actual fixed
  case and the direct regression test that now covers it.
- The safer intended behavior was already documented: dead paused sessions stay blocked and rely on
  the manual retry path. This turn only brought the code in line with that documentation.
- GitHub Actions `quality` passed for the exact current PR #101 head
  `bee2babd4608d5ef4ddf7497c979d5e1c76111ff` in run `24173386494` at
  `2026-04-09T05:09:57Z`.
- Direct Linear GraphQL lookups with the injected `LINEAR_API_KEY` could not resolve issue
  `14038435-d47c-4a74-8951-1e2312456bef` or workpad comment
  `120f9183-00f1-4b0b-af69-a4850e6543cd`; both returned `Entity not found`, so remote workpad
  mutation is unavailable from this session.
- The validation set stayed deterministic and local; no live Linear mutation was required to prove
  stale-worker reconciliation behavior.
- Ticket-local evidence lives here and reviewer-facing evidence lives in
  `artifacts/validation/ENG-10744.md`.
- PR #101 had no actionable review feedback as of `2026-04-09T05:06:28Z`: the top-level comment
  list only contained the Linear linkback, `gh api .../pulls/101/comments` returned no inline
  review comments, and `gh pr view 101 --json reviews` returned no review summaries.

## Review Notes

### Context Checkpoint

- Decisions made:
  - fixed reconciliation ordering instead of narrowing docs so dead paused sessions reach
    `reconcile_stale_worker_session(...)`
  - kept paused sessions non-retryable; the delivered behavior is terminal blocked
    `paused worker died`
  - used a `reconcile_sessions()` regression test so the outer control-flow bug is covered directly
  - reran the stale-worker validation set, review check, `clippy`, and full `make quality`, then
    waited for the exact current PR head to pass GitHub Actions `quality`
- Failed approaches:
  - the earlier retention branch returned all `Paused` sessions before stale-worker classification,
    so helper-level stale-worker tests alone were not enough to catch the gap
- Blockers:
  - no local code, validation, or current-head GitHub verification blocker remains
  - remote workpad sync is blocked because the configured Linear API cannot resolve issue
    `14038435-d47c-4a74-8951-1e2312456bef` or workpad comment
    `120f9183-00f1-4b0b-af69-a4850e6543cd`
- Exact remaining work:
  - refresh remote-only PR/workpad metadata from an auth/profile path that can resolve the issue
    and workpad ids above
- Checklist and validation status:
  - `cargo test reconcile_sessions_blocks_paused_session_with_dead_worker_pid --lib` passed
  - `cargo test stale_running_session_relaunches_with_existing_context_and_fresh_pid --lib` passed
  - `cargo test stale_running_session_budget_exhaustion_blocks_without_relaunch --lib` passed
  - `cargo test stale_worker_recovery_persists_metadata_before_replacement_worker_write --lib`
    passed
  - `cargo test listen::store --lib` passed
  - `cargo test listen::dashboard --lib` passed
  - `cargo test listen::mod --lib` passed
  - `cargo test --test listen -- --test-threads=1` passed
  - `cargo test listen_surfaces_no_checks_configured_in_inspect_and_dashboard --test listen -- --exact`
    passed
  - `cargo run -- agents review --check --root .` passed
  - `cargo clippy --all-targets --all-features -- -D warnings` passed
  - `make quality` passed locally
  - `gh pr view 101 --repo 0xIntuition/intuition-harness-cli --comments` showed only the Linear
    linkback comment and no actionable top-level review feedback
  - `gh api repos/0xIntuition/intuition-harness-cli/pulls/101/comments --paginate` returned no
    inline review comments
  - `gh pr view 101 --repo 0xIntuition/intuition-harness-cli --json reviews` returned no review
    summaries
  - `gh pr view 101 --repo 0xIntuition/intuition-harness-cli --json headRefOid,statusCheckRollup,updatedAt,url`
    showed PR #101 head `bee2babd4608d5ef4ddf7497c979d5e1c76111ff` with `quality`
    `COMPLETED` and `SUCCESS`
  - `gh run view 24173386494 --repo 0xIntuition/intuition-harness-cli --json status,conclusion,headSha,url,workflowName,updatedAt`
    showed the exact-head `quality` workflow completed with `success`
  - direct Linear GraphQL `query Issue($id)` and `query Comment($id)` calls against the injected
    `LINEAR_API_URL` both returned `Entity not found` for the current ticket/workpad ids

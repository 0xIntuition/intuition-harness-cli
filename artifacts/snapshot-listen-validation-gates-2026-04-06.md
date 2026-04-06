# Snapshot: Listen Validation Gates

Owner: Codex

Date: 2026-04-06

Ticket: ENG-10505

## Purpose

Capture the branch-local proof for shared validation-profile resolution plus the enforced
pre-PR and post-publication CI repair gates used by `meta agents listen`.

## Scope and Non-Goals

- In scope: shared validation-profile selection, repo-scoped listen validation config, explicit
  `Validating` session state, enforced pre-PR validation, and bounded CI repair on the same branch PR.
- Out of scope: hook installation and follow-up PR linting automation.

## Acceptance Mapping

- Shared resolver precedence is implemented in `src/validation.rs` and exercised by
  `tests/merge.rs::merge_uses_repo_configured_validation_profile_before_heuristics`.
- Repo-scoped validation settings load through `PlanningMeta.validation` in `src/config.rs`.
- `SessionPhase::Validating` is represented in `src/listen/state.rs`,
  `src/listen/dashboard.rs`, and `tests/listen.rs::listen_sessions_inspect_renders_validating_phase`.
- Pre-PR validation blocks PR mutation and re-enters the loop in
  `src/listen/worker.rs::run_pre_pr_validation_gate` and
  `tests/listen.rs::listen_worker_retries_failed_pre_pr_validation_and_blocks_when_budget_is_exhausted`.
- Post-publication CI repair reuses the same PR in `src/listen/worker.rs` and
  `tests/listen.rs::listen_worker_repairs_failing_pr_checks_and_reuses_the_same_pull_request`.
- Operator diagnostics for the selected validation profile are surfaced by
  `tests/listen.rs::listen_check_reports_codex_config_status_and_linear_api_validation`.

## Notes

- The local ENG-10292 packet referenced in the ticket text was not present in this workspace, so no
  packet-local sync was possible on this branch.
- Validation evidence for the final branch state is recorded in `../validation.md`.

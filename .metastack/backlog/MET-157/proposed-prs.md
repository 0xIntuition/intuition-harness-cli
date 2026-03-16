# Proposed PRs: Technical: Add issue readiness scoring and safe multi-ticket kickoff for small teams

Last updated: 2026-03-16

## PR Strategy

- Keep command-contract changes reviewable before batch kickoff behavior lands.
- Avoid mixing readiness behavior with unrelated `listen` lifecycle refactors.
- Land docs and tests with the behavior they describe.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| technical-add-issue-readiness-scoring-and-safe-multi-ticket-kickoff-for-small-teams-01 | Add readiness evaluator contract, CLI surface, and config plumbing | `src/cli.rs`, `src/lib.rs`, `src/config.rs`, `src/setup.rs`, `src/listen/state.rs`, `src/listen/mod.rs` | None | Medium | `unassigned` | planned |
| technical-add-issue-readiness-scoring-and-safe-multi-ticket-kickoff-for-small-teams-02 | Integrate bounded listen pickup, session persistence, and workspace-safe kickoff | `src/listen/mod.rs`, `src/listen/store.rs`, `src/listen/workspace.rs`, `src/listen/dashboard.rs`, `src/listen/web.rs`, `src/linear/*` | technical-add-issue-readiness-scoring-and-safe-multi-ticket-kickoff-for-small-teams-01 | High | `unassigned` | planned |
| technical-add-issue-readiness-scoring-and-safe-multi-ticket-kickoff-for-small-teams-03 | Finalize docs, dashboard wording, and regression coverage | `README.md`, `docs/agent-daemon.md`, `tests/cli.rs`, `src/listen/*` unit tests | technical-add-issue-readiness-scoring-and-safe-multi-ticket-kickoff-for-small-teams-02 | Medium | `unassigned` | planned |

## Merge Order

1. `technical-add-issue-readiness-scoring-and-safe-multi-ticket-kickoff-for-small-teams-01`
2. `technical-add-issue-readiness-scoring-and-safe-multi-ticket-kickoff-for-small-teams-02`
3. `technical-add-issue-readiness-scoring-and-safe-multi-ticket-kickoff-for-small-teams-03`

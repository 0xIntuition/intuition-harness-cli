# Risks: Technical: Implement the end-to-end `meta merge` command, dashboard, agent merge-run orchestration, and aggregate PR publication

Last updated: 2026-03-16

## Active Risks

| Risk | Impact | Likelihood | Mitigation | Owner | Status |
|---|---|---|---|---|---|
| `gh` is missing, unauthenticated, or cannot resolve the active repository on contributor machines | High | Medium | Add explicit preflight checks, actionable errors, and stub-backed tests for each failure mode | `child issue driver` | open |
| Merge execution accidentally mutates the source checkout instead of an isolated workspace | High | Low | Reuse or extract workspace-safety checks from `src/listen/workspace.rs` and add direct tests for source-repo protection | `child issue driver` | open |
| Agent prompts do not provide enough repo or PR context for stable merge ordering and conflict recovery | High | Medium | Define the prompt contract early, persist the generated plan in run artifacts, and add fixture-based prompt assertions | `child issue driver` | open |
| Dashboard interaction semantics drift from test coverage and become hard to review | Medium | Medium | Keep scripted-event `--render-once` snapshots for empty, single, multi, and cancel states | `child issue driver` | open |
| Validation gating is too slow or too opaque for repeated merge batches | Medium | Medium | Make validation commands explicit, persist results in artifacts, and use deterministic temp-repo proofs for pass or fail behavior | `child issue driver` | open |

## Open Questions

1. Should repository validation default to `make all`, or should `meta merge` introduce a merge-specific validation list in repo-scoped config?
2. Should aggregate branch names be purely run-id based, or should they include a stable slug derived from the selected PRs for easier reuse?
3. How much of the conflict-resolution transcript should be stored under `.metastack/merge-runs/<RUN_ID>/` without turning artifacts into raw log dumps?

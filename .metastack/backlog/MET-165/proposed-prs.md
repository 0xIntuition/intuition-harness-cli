# Proposed PRs: Technical: Implement the end-to-end `meta merge` command, dashboard, agent merge-run orchestration, and aggregate PR publication

Last updated: 2026-03-16

## PR Strategy

- Keep each PR independently reviewable and testable.
- Land the command and transport contract before the runner and publication layers.
- Preserve existing command-family behavior outside the new `meta merge` surface.
- Keep docs and prompt updates close to the implementation slice that changes behavior.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| technical-implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-01 | Add `meta merge` CLI wiring plus the `gh` discovery contract and JSON proof path | `src/cli.rs`, `src/lib.rs`, `src/merge/*`, `tests/merge.rs`, `tests/commands.rs` | None | Medium | `repo maintainer` | planned |
| technical-implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-02 | Implement the one-shot dashboard, selection summary, and deterministic snapshot coverage | `src/merge/dashboard.rs`, `src/cli.rs`, `tests/merge.rs` | technical-implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-01 | Medium | `repo maintainer` | planned |
| technical-implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-03 | Add safe workspace execution, merge-run artifacts, agent planning, conflict handling, and validation gating | `src/merge/runner.rs`, `src/fs.rs`, `src/listen/workspace.rs`, `src/agents/execution.rs`, `prompts/injected-agent-workflow-contract.md`, `tests/merge.rs` | technical-implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-02 | High | `repo maintainer` | planned |
| technical-implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-04 | Finish aggregate PR publication, docs, and the end-to-end hermetic proof | `src/merge/*`, `README.md`, `WORKFLOW.md`, `tests/merge.rs`, `tests/support/common.rs` | technical-implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-03 | Medium | `repo maintainer` | planned |

## Merge Order

1. `technical-implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-01`
2. `technical-implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-02`
3. `technical-implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-03`
4. `technical-implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-04`

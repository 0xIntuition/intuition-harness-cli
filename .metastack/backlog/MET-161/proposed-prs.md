# Proposed PRs: Implement the end-to-end `meta merge` command, dashboard, agent merge-run orchestration, and aggregate PR publication

Last updated: 2026-03-16

## PR Strategy

- Keep each PR independently reviewable.
- Land contract changes before consumer migration PRs.
- Avoid mixing behavior changes with broad refactors.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-01 | Lock contract surface | `TBD` | None | Medium | `@tbd` | planned |
| implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-02 | Implement core behavior | `TBD` | implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-01 | Medium | `@tbd` | planned |
| implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-03 | Consumer alignment + tests | `TBD` | implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-02 | Low | `@tbd` | planned |

## Merge Order

1. `implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-01`
2. `implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-02`
3. `implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run-orchestration-and-aggregate-pr-publication-03`

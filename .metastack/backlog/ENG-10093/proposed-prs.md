# Proposed PRs: Open dashboards immediately and load Linear data asynchronously across listen, backlog tech, and backlog sync

Last updated: 2026-03-16

## PR Strategy

- Keep each PR independently reviewable.
- Land contract changes before consumer migration PRs.
- Avoid mixing behavior changes with broad refactors.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| open-dashboards-immediately-and-load-linear-data-asynchronously-across-listen-backlog-tech-and-backlog-sync-01 | Lock contract surface | `TBD` | None | Medium | `@tbd` | planned |
| open-dashboards-immediately-and-load-linear-data-asynchronously-across-listen-backlog-tech-and-backlog-sync-02 | Implement core behavior | `TBD` | open-dashboards-immediately-and-load-linear-data-asynchronously-across-listen-backlog-tech-and-backlog-sync-01 | Medium | `@tbd` | planned |
| open-dashboards-immediately-and-load-linear-data-asynchronously-across-listen-backlog-tech-and-backlog-sync-03 | Consumer alignment + tests | `TBD` | open-dashboards-immediately-and-load-linear-data-asynchronously-across-listen-backlog-tech-and-backlog-sync-02 | Low | `@tbd` | planned |

## Merge Order

1. `open-dashboards-immediately-and-load-linear-data-asynchronously-across-listen-backlog-tech-and-backlog-sync-01`
2. `open-dashboards-immediately-and-load-linear-data-asynchronously-across-listen-backlog-tech-and-backlog-sync-02`
3. `open-dashboards-immediately-and-load-linear-data-asynchronously-across-listen-backlog-tech-and-backlog-sync-03`

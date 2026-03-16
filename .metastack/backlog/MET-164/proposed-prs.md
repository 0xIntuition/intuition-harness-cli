# Proposed PRs: Add optional per-ticket agent selection for `meta agents listen` pickups

Last updated: 2026-03-16

## PR Strategy

- Keep each PR independently reviewable.
- Land contract changes before consumer migration PRs.
- Avoid mixing behavior changes with broad refactors.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| add-optional-per-ticket-agent-selection-for-meta-agents-listen-pickups-01 | Lock contract surface | `TBD` | None | Medium | `@tbd` | planned |
| add-optional-per-ticket-agent-selection-for-meta-agents-listen-pickups-02 | Implement core behavior | `TBD` | add-optional-per-ticket-agent-selection-for-meta-agents-listen-pickups-01 | Medium | `@tbd` | planned |
| add-optional-per-ticket-agent-selection-for-meta-agents-listen-pickups-03 | Consumer alignment + tests | `TBD` | add-optional-per-ticket-agent-selection-for-meta-agents-listen-pickups-02 | Low | `@tbd` | planned |

## Merge Order

1. `add-optional-per-ticket-agent-selection-for-meta-agents-listen-pickups-01`
2. `add-optional-per-ticket-agent-selection-for-meta-agents-listen-pickups-02`
3. `add-optional-per-ticket-agent-selection-for-meta-agents-listen-pickups-03`

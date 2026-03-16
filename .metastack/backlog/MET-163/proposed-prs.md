# Proposed PRs: Add advanced agent-routing config, resolver, config surfaces, and route-aware execution across agent-backed commands

Last updated: 2026-03-16

## PR Strategy

- Keep each PR independently reviewable.
- Land contract changes before consumer migration PRs.
- Avoid mixing behavior changes with broad refactors.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| add-advanced-agent-routing-config-resolver-config-surfaces-and-route-aware-execution-across-agent-backed-commands-01 | Lock contract surface | `TBD` | None | Medium | `@tbd` | planned |
| add-advanced-agent-routing-config-resolver-config-surfaces-and-route-aware-execution-across-agent-backed-commands-02 | Implement core behavior | `TBD` | add-advanced-agent-routing-config-resolver-config-surfaces-and-route-aware-execution-across-agent-backed-commands-01 | Medium | `@tbd` | planned |
| add-advanced-agent-routing-config-resolver-config-surfaces-and-route-aware-execution-across-agent-backed-commands-03 | Consumer alignment + tests | `TBD` | add-advanced-agent-routing-config-resolver-config-surfaces-and-route-aware-execution-across-agent-backed-commands-02 | Low | `@tbd` | planned |

## Merge Order

1. `add-advanced-agent-routing-config-resolver-config-surfaces-and-route-aware-execution-across-agent-backed-commands-01`
2. `add-advanced-agent-routing-config-resolver-config-surfaces-and-route-aware-execution-across-agent-backed-commands-02`
3. `add-advanced-agent-routing-config-resolver-config-surfaces-and-route-aware-execution-across-agent-backed-commands-03`

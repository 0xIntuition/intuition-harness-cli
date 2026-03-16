# Proposed PRs: Fix shared TUI input behavior and standardize loading, cursor, and active-state cues across interactive dashboards

Last updated: 2026-03-16

## PR Strategy

- Keep each PR independently reviewable.
- Land contract changes before consumer migration PRs.
- Avoid mixing behavior changes with broad refactors.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| fix-shared-tui-input-behavior-and-standardize-loading-cursor-and-active-state-cues-across-interactive-dashboards-01 | Lock contract surface | `TBD` | None | Medium | `@tbd` | planned |
| fix-shared-tui-input-behavior-and-standardize-loading-cursor-and-active-state-cues-across-interactive-dashboards-02 | Implement core behavior | `TBD` | fix-shared-tui-input-behavior-and-standardize-loading-cursor-and-active-state-cues-across-interactive-dashboards-01 | Medium | `@tbd` | planned |
| fix-shared-tui-input-behavior-and-standardize-loading-cursor-and-active-state-cues-across-interactive-dashboards-03 | Consumer alignment + tests | `TBD` | fix-shared-tui-input-behavior-and-standardize-loading-cursor-and-active-state-cues-across-interactive-dashboards-02 | Low | `@tbd` | planned |

## Merge Order

1. `fix-shared-tui-input-behavior-and-standardize-loading-cursor-and-active-state-cues-across-interactive-dashboards-01`
2. `fix-shared-tui-input-behavior-and-standardize-loading-cursor-and-active-state-cues-across-interactive-dashboards-02`
3. `fix-shared-tui-input-behavior-and-standardize-loading-cursor-and-active-state-cues-across-interactive-dashboards-03`

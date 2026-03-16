# Proposed PRs: Redesign config and setup TUI layout so sidebar content stays readable without overflow

Last updated: 2026-03-16

## PR Strategy

- Keep each PR independently reviewable.
- Land contract changes before consumer migration PRs.
- Avoid mixing behavior changes with broad refactors.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| redesign-config-and-setup-tui-layout-so-sidebar-content-stays-readable-without-overflow-01 | Lock contract surface | `TBD` | None | Medium | `@tbd` | planned |
| redesign-config-and-setup-tui-layout-so-sidebar-content-stays-readable-without-overflow-02 | Implement core behavior | `TBD` | redesign-config-and-setup-tui-layout-so-sidebar-content-stays-readable-without-overflow-01 | Medium | `@tbd` | planned |
| redesign-config-and-setup-tui-layout-so-sidebar-content-stays-readable-without-overflow-03 | Consumer alignment + tests | `TBD` | redesign-config-and-setup-tui-layout-so-sidebar-content-stays-readable-without-overflow-02 | Low | `@tbd` | planned |

## Merge Order

1. `redesign-config-and-setup-tui-layout-so-sidebar-content-stays-readable-without-overflow-01`
2. `redesign-config-and-setup-tui-layout-so-sidebar-content-stays-readable-without-overflow-02`
3. `redesign-config-and-setup-tui-layout-so-sidebar-content-stays-readable-without-overflow-03`

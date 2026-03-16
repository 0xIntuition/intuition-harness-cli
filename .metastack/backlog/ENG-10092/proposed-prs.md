# Proposed PRs: Standardize TUI multiline inputs to submit on Enter and newline on Shift+Enter

Last updated: 2026-03-16

## PR Strategy

- Keep each PR independently reviewable.
- Land contract changes before consumer migration PRs.
- Avoid mixing behavior changes with broad refactors.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| standardize-tui-multiline-inputs-to-submit-on-enter-and-newline-on-shift-enter-01 | Lock contract surface | `TBD` | None | Medium | `@tbd` | planned |
| standardize-tui-multiline-inputs-to-submit-on-enter-and-newline-on-shift-enter-02 | Implement core behavior | `TBD` | standardize-tui-multiline-inputs-to-submit-on-enter-and-newline-on-shift-enter-01 | Medium | `@tbd` | planned |
| standardize-tui-multiline-inputs-to-submit-on-enter-and-newline-on-shift-enter-03 | Consumer alignment + tests | `TBD` | standardize-tui-multiline-inputs-to-submit-on-enter-and-newline-on-shift-enter-02 | Low | `@tbd` | planned |

## Merge Order

1. `standardize-tui-multiline-inputs-to-submit-on-enter-and-newline-on-shift-enter-01`
2. `standardize-tui-multiline-inputs-to-submit-on-enter-and-newline-on-shift-enter-02`
3. `standardize-tui-multiline-inputs-to-submit-on-enter-and-newline-on-shift-enter-03`

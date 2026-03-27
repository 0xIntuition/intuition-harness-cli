# Proposed PRs: Update the README.md

Last updated: 2026-03-27

## PR Strategy

- Keep each PR independently reviewable.
- Land contract changes before consumer migration PRs.
- Avoid mixing behavior changes with broad refactors.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| update-the-readme-md-01 | Lock contract surface | `TBD` | None | Medium | `@tbd` | planned |
| update-the-readme-md-02 | Implement core behavior | `TBD` | update-the-readme-md-01 | Medium | `@tbd` | planned |
| update-the-readme-md-03 | Consumer alignment + tests | `TBD` | update-the-readme-md-02 | Low | `@tbd` | planned |

## Merge Order

1. `update-the-readme-md-01`
2. `update-the-readme-md-02`
3. `update-the-readme-md-03`

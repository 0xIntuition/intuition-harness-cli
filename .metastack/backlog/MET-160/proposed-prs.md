# Proposed PRs: Add explicit automation permissions, dry-run tooling, and starter templates for powerful but safe repo-local jobs

Last updated: 2026-03-16

## PR Strategy

- Keep each PR independently reviewable.
- Land contract changes before consumer migration PRs.
- Avoid mixing behavior changes with broad refactors.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| add-explicit-automation-permissions-dry-run-tooling-and-starter-templates-for-powerful-but-safe-repo-local-jobs-01 | Lock contract surface | `TBD` | None | Medium | `@tbd` | planned |
| add-explicit-automation-permissions-dry-run-tooling-and-starter-templates-for-powerful-but-safe-repo-local-jobs-02 | Implement core behavior | `TBD` | add-explicit-automation-permissions-dry-run-tooling-and-starter-templates-for-powerful-but-safe-repo-local-jobs-01 | Medium | `@tbd` | planned |
| add-explicit-automation-permissions-dry-run-tooling-and-starter-templates-for-powerful-but-safe-repo-local-jobs-03 | Consumer alignment + tests | `TBD` | add-explicit-automation-permissions-dry-run-tooling-and-starter-templates-for-powerful-but-safe-repo-local-jobs-02 | Low | `@tbd` | planned |

## Merge Order

1. `add-explicit-automation-permissions-dry-run-tooling-and-starter-templates-for-powerful-but-safe-repo-local-jobs-01`
2. `add-explicit-automation-permissions-dry-run-tooling-and-starter-templates-for-powerful-but-safe-repo-local-jobs-02`
3. `add-explicit-automation-permissions-dry-run-tooling-and-starter-templates-for-powerful-but-safe-repo-local-jobs-03`

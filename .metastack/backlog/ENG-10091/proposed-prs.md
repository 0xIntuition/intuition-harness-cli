# Proposed PRs: Add reusable long-running progress tracking and a live merge progress screen to `meta merge`

Last updated: 2026-03-16

## PR Strategy

- Keep each PR independently reviewable.
- Land contract changes before consumer migration PRs.
- Avoid mixing behavior changes with broad refactors.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| add-reusable-long-running-progress-tracking-and-a-live-merge-progress-screen-to-meta-merge-01 | Lock contract surface | `TBD` | None | Medium | `@tbd` | planned |
| add-reusable-long-running-progress-tracking-and-a-live-merge-progress-screen-to-meta-merge-02 | Implement core behavior | `TBD` | add-reusable-long-running-progress-tracking-and-a-live-merge-progress-screen-to-meta-merge-01 | Medium | `@tbd` | planned |
| add-reusable-long-running-progress-tracking-and-a-live-merge-progress-screen-to-meta-merge-03 | Consumer alignment + tests | `TBD` | add-reusable-long-running-progress-tracking-and-a-live-merge-progress-screen-to-meta-merge-02 | Low | `@tbd` | planned |

## Merge Order

1. `add-reusable-long-running-progress-tracking-and-a-live-merge-progress-screen-to-meta-merge-01`
2. `add-reusable-long-running-progress-tracking-and-a-live-merge-progress-screen-to-meta-merge-02`
3. `add-reusable-long-running-progress-tracking-and-a-live-merge-progress-screen-to-meta-merge-03`

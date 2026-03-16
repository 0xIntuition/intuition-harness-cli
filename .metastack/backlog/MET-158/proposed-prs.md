# Proposed PRs: Introduce a versioned automation job schema that extends `meta runtime cron` beyond schedule-only jobs

Last updated: 2026-03-16

## PR Strategy

- Keep each PR independently reviewable.
- Land contract changes before consumer migration PRs.
- Avoid mixing behavior changes with broad refactors.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| introduce-a-versioned-automation-job-schema-that-extends-meta-runtime-cron-beyond-schedule-only-jobs-01 | Lock contract surface | `TBD` | None | Medium | `@tbd` | planned |
| introduce-a-versioned-automation-job-schema-that-extends-meta-runtime-cron-beyond-schedule-only-jobs-02 | Implement core behavior | `TBD` | introduce-a-versioned-automation-job-schema-that-extends-meta-runtime-cron-beyond-schedule-only-jobs-01 | Medium | `@tbd` | planned |
| introduce-a-versioned-automation-job-schema-that-extends-meta-runtime-cron-beyond-schedule-only-jobs-03 | Consumer alignment + tests | `TBD` | introduce-a-versioned-automation-job-schema-that-extends-meta-runtime-cron-beyond-schedule-only-jobs-02 | Low | `@tbd` | planned |

## Merge Order

1. `introduce-a-versioned-automation-job-schema-that-extends-meta-runtime-cron-beyond-schedule-only-jobs-01`
2. `introduce-a-versioned-automation-job-schema-that-extends-meta-runtime-cron-beyond-schedule-only-jobs-02`
3. `introduce-a-versioned-automation-job-schema-that-extends-meta-runtime-cron-beyond-schedule-only-jobs-03`

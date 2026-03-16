# Proposed PRs: Fix `meta backlog plan` session state loss after follow-up submission and generation

Last updated: 2026-03-16

## PR Strategy

- Keep each PR independently reviewable.
- Land contract changes before consumer migration PRs.
- Avoid mixing behavior changes with broad refactors.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| fix-meta-backlog-plan-session-state-loss-after-follow-up-submission-and-generation-01 | Lock contract surface | `TBD` | None | Medium | `@tbd` | planned |
| fix-meta-backlog-plan-session-state-loss-after-follow-up-submission-and-generation-02 | Implement core behavior | `TBD` | fix-meta-backlog-plan-session-state-loss-after-follow-up-submission-and-generation-01 | Medium | `@tbd` | planned |
| fix-meta-backlog-plan-session-state-loss-after-follow-up-submission-and-generation-03 | Consumer alignment + tests | `TBD` | fix-meta-backlog-plan-session-state-loss-after-follow-up-submission-and-generation-02 | Low | `@tbd` | planned |

## Merge Order

1. `fix-meta-backlog-plan-session-state-loss-after-follow-up-submission-and-generation-01`
2. `fix-meta-backlog-plan-session-state-loss-after-follow-up-submission-and-generation-02`
3. `fix-meta-backlog-plan-session-state-loss-after-follow-up-submission-and-generation-03`

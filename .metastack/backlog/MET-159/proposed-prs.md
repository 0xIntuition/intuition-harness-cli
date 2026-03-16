# Proposed PRs: Add composable automation steps so one job can run commands, workflows, and agents in a controlled sequence

Last updated: 2026-03-16

## PR Strategy

- Keep each PR independently reviewable.
- Land contract changes before consumer migration PRs.
- Avoid mixing behavior changes with broad refactors.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| add-composable-automation-steps-so-one-job-can-run-commands-workflows-and-agents-in-a-controlled-sequence-01 | Lock contract surface | `TBD` | None | Medium | `@tbd` | planned |
| add-composable-automation-steps-so-one-job-can-run-commands-workflows-and-agents-in-a-controlled-sequence-02 | Implement core behavior | `TBD` | add-composable-automation-steps-so-one-job-can-run-commands-workflows-and-agents-in-a-controlled-sequence-01 | Medium | `@tbd` | planned |
| add-composable-automation-steps-so-one-job-can-run-commands-workflows-and-agents-in-a-controlled-sequence-03 | Consumer alignment + tests | `TBD` | add-composable-automation-steps-so-one-job-can-run-commands-workflows-and-agents-in-a-controlled-sequence-02 | Low | `@tbd` | planned |

## Merge Order

1. `add-composable-automation-steps-so-one-job-can-run-commands-workflows-and-agents-in-a-controlled-sequence-01`
2. `add-composable-automation-steps-so-one-job-can-run-commands-workflows-and-agents-in-a-controlled-sequence-02`
3. `add-composable-automation-steps-so-one-job-can-run-commands-workflows-and-agents-in-a-controlled-sequence-03`

# Proposed PRs: Technical: Technical: Add multi-provider registry, approval profiles, and budget governance for fleet operations

Last updated: 2026-03-16

## PR Strategy

- Keep config-contract work separate from runtime-behavior work so reviewers can validate compatibility first.
- Land command-surface and JSON-output work before wiring every agent-backed flow through the new resolver.
- Keep persistence and budget rollup changes isolated enough that reviewers can compare the event model against command output.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| technical-technical-add-multi-provider-registry-approval-profiles-and-budget-governance-for-fleet-operations-01 | Lock additive config contract and compatibility normalization | `src/config.rs`, `src/config_command.rs`, `src/setup.rs`, `src/fs.rs`, `tests/config.rs` | None | Medium | `runtime-config` | planned |
| technical-technical-add-multi-provider-registry-approval-profiles-and-budget-governance-for-fleet-operations-02 | Ship provider and policy operator surfaces | `src/cli.rs`, `src/lib.rs`, new provider/policy modules, `tests/commands.rs`, `tests/workflows.rs` | technical-technical-add-multi-provider-registry-approval-profiles-and-budget-governance-for-fleet-operations-01 | Medium | `cli-runtime` | planned |
| technical-technical-add-multi-provider-registry-approval-profiles-and-budget-governance-for-fleet-operations-03 | Route runtime launches, persist events, add budgets status, and finish docs | `src/agents/execution.rs`, `src/workflows.rs`, `src/listen/*`, `src/plan.rs`, `src/technical.rs`, `README.md`, `WORKFLOW.md`, `tests/listen.rs`, `tests/plan.rs`, `tests/technical.rs` | technical-technical-add-multi-provider-registry-approval-profiles-and-budget-governance-for-fleet-operations-02 | High | `runtime-governance` | planned |

## Merge Order

1. `technical-technical-add-multi-provider-registry-approval-profiles-and-budget-governance-for-fleet-operations-01`
2. `technical-technical-add-multi-provider-registry-approval-profiles-and-budget-governance-for-fleet-operations-02`
3. `technical-technical-add-multi-provider-registry-approval-profiles-and-budget-governance-for-fleet-operations-03`

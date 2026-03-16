# Aggregate merge for `0xIntuition/metastack-cli`

Run ID: `20260316T210116Z`

Included pull requests:
- #1 ENG-10063: add advanced agent routing (https://github.com/0xIntuition/metastack-cli/pull/1)

Planner summary:
Only PR #1 is in scope, so the safest explicit order is to merge it alone. Its highest-risk conflict areas are the shared config and command wiring surfaces it changes heavily: route validation and resolution in `src/config.rs`, the expanded config UI/CLI in `src/config_command.rs` and `src/cli.rs`, and the agent-backed command entrypoints in `src/scan.rs`, `src/workflows.rs`, `src/cron.rs`, `src/merge.rs`, and `src/plan.rs`, plus `README.md` for behavior docs.

Conflict hotspots called out before execution:
- src/config.rs
- src/config_command.rs
- src/cli.rs
- README.md
- src/scan.rs
- src/workflows.rs
- src/cron.rs
- src/merge.rs
- src/plan.rs
# Implementation Plan

## Workstreams

1. Define the additive config schema for provider registry, model metadata, routing rules, approval profiles, and budget profiles.
2. Add the operator command surfaces and JSON output contracts for provider inspection, policy inspection/application, and budget status.
3. Route every agent-backed launch through a shared runtime resolver and persist effective provider, policy, and cost evidence.
4. Update docs and integration tests so the new governance layer is discoverable and compatibility-safe.

## Likely Touchpoints

- CLI entrypoints: `src/cli.rs`, `src/lib.rs`
- Config and validation: `src/config.rs`, `src/config_command.rs`, `src/setup.rs`
- Agent runtime: `src/agents/execution.rs`, `src/agents/mod.rs`, `src/workflows.rs`, `src/scan.rs`, `src/plan.rs`, `src/technical.rs`, `src/listen/mod.rs`, `src/listen/state.rs`, `src/listen/store.rs`
- Filesystem helpers: `src/fs.rs`
- Documentation: `README.md`, `WORKFLOW.md`, `docs/agent-daemon.md` if the listen/dashboard surface changes materially
- Tests: `tests/config.rs`, `tests/commands.rs`, `tests/workflows.rs`, `tests/listen.rs`, plus targeted additions for any new command modules

## Proposed Module Boundaries

- Shared runtime registry/resolution module for provider/model/policy selection.
- Provider diagnostics module for list/test/doctor surfaces.
- Policy module for effective approval and budget profile inspection plus repo-local apply behavior.
- Budget/event module for ledger writes, rollups, and status rendering.

## Validation Strategy

- Unit-test config parsing, compatibility normalization, routing resolution, and event rollups.
- Add command-level integration tests for the new subcommands with both human-readable and `--json` output.
- Add targeted proofs for success and failure paths: unsupported model, missing provider executable, failed health check, over-budget run, and fallback activation.
- Prove compatibility by running an existing single-agent fixture through the new resolver without changing its selected provider/model.

# Technical: Technical: Add multi-provider registry, approval profiles, and budget governance for fleet operations

## Parent Issue

- Identifier: `MET-106`
- Title: Technical: Add multi-provider registry, approval profiles, and budget governance for fleet operations
- URL: https://linear.app/metastack-backlog/issue/MET-106/technical-add-multi-provider-registry-approval-profiles-and-budget

## Parent Feature Context

- Parent feature: `MET-94` - Add multi-provider registry, approval profiles, and budget governance for fleet operations
- Repository scope: `metastack-cli` only
- Current foundation in this repo: `src/config.rs` already resolves install-scoped and repo-scoped agent settings, `src/agents/execution.rs` centralizes launch behavior, `.metastack/agents/sessions/` stores runtime artifacts, and `tests/` already provide command-level coverage for config, workflows, scan, plan, technical, and listen flows.

## Context

The current CLI resolves one effective provider/model pair from a mix of install-scoped config, repo-local `.metastack/meta.json`, and command-line overrides. That is enough for today's local execution flows, but it does not give operators a durable registry of available providers, an operator-visible approval profile, or a cost/budget model that can be queried after the run. The result is that runtime governance is implicit in code and difficult to inspect from the CLI.

This technical slice turns the existing agent runtime into a first-pass fleet governance layer for a single repository checkout. It should stay additive, preserve current single-agent behavior, and make routing, fallback, approval posture, and spend evidence visible through deterministic local commands.

## Proposed Approach

1. Extend config parsing and validation so install-scoped config can describe multiple providers, model metadata, routing rules, approval profiles, and budget profiles while repo-local `.metastack/meta.json` can select the effective provider/policy for this repository.
2. Add explicit operator surfaces for provider inspection, policy inspection/application, and budget status, each with text and `--json` output.
3. Route agent-backed commands through one shared resolution layer so backlog, workflow, scan, and listen runs record the effective provider, model, routing source, approval profile, and fallback reason.
4. Persist usage, token, and estimated cost events in a local append-only ledger under `.metastack/agents/sessions/` so status commands and dashboards do not depend on parsing raw logs.
5. Update docs and tests together so operators understand the new config contract and reviewers can validate the compatibility path.

## In Scope

- Additive provider registry and routing configuration in install-scoped config and repo-local metadata.
- Provider capability and health inspection surfaces.
- Approval-profile inspection and repo-local application.
- Budget profile selection, status reporting, and local usage/cost persistence.
- Integration coverage and docs for the new command paths.

## Out of Scope

- Hosted control-plane orchestration or cross-host scheduling.
- External billing settlement, provider invoices, or remote budget storage.
- Replacing `meta agents listen` with a new daemon architecture.
- Changing Linear workflow semantics unrelated to provider/policy selection.

## Milestones

1. Define the config contract and compatibility normalization.
2. Ship read-only provider/policy/budget inspection commands.
3. Route runtime launches through the shared resolver and record effective governance metadata.
4. Add budget event persistence and budget status reporting.
5. Finish docs, command proofs, and compatibility evidence.

## Definition of Done

- Configuration supports multiple providers and models with explicit routing and fallback rules.
- Operators can inspect provider health, available capabilities, and the active approval and budget profiles for a repository run.
- Usage, token, and cost events are persisted in a durable local format that powers `--json` output and dashboards.
- Existing single-agent configurations continue to work without manual migration.
- [`../../../README.md`](../../../README.md) and [`../../../WORKFLOW.md`](../../../WORKFLOW.md) document the new config and operator workflows.

## Risks

- Risk: runtime selection logic leaks across multiple commands.
  Mitigation: put provider/model/policy resolution behind one shared module and keep command handlers thin.
- Risk: approval and budget precedence becomes ambiguous between install-scoped config and repo-local overrides.
  Mitigation: surface the effective policy and the source of each value in `meta policy show` and JSON output.
- Risk: budget status drifts from runtime logging.
  Mitigation: use one append-only event ledger as the source of truth for status commands and dashboards.
- Risk: provider health checks become flaky when they rely on network access.
  Mitigation: keep `doctor` mostly static and make active provider tests an explicit opt-in command.

## Validation

- [ ] `cargo test`
- [ ] Focused CLI command proofs for `meta providers list|test|doctor`
- [ ] Focused CLI command proofs for `meta policy show|apply`
- [ ] Focused CLI command proofs for `meta budgets status`
- [ ] Config compatibility proof for an existing single-agent setup
- [ ] Filesystem evidence for persisted usage, token, and cost events

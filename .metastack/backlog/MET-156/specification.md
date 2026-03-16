# Specification: Technical: Technical: Add multi-provider registry, approval profiles, and budget governance for fleet operations

Version: 0.1  
Last updated: 2026-03-16

Parent index: [`./index.md`](./index.md)

## 1. Executive Summary

Add a repository-scoped fleet-governance layer to `metastack-cli` so operators can configure multiple local agent providers, inspect effective approval and budget policy, and query durable usage/cost evidence after runs. The implementation must preserve the current single-agent workflow while making provider resolution, fallback, and budget posture explicit through new CLI commands and local runtime metadata.

## 2. Problem Statement

- Problem: current runtime behavior is centered on one resolved provider/model pair and embeds approval posture in provider-specific launch code, which makes provider choice, fallback behavior, and spend posture hard to inspect.
- Why now: the repository already has additive config loading, centralized launch code, repo-local metadata, and session storage; adding governance on top of those primitives is lower risk than inventing a parallel runtime system later.
- Non-goals:
  - hosted orchestration or multi-host coordination
  - external billing settlement or provider invoice reconciliation
  - a rewrite of `meta agents listen` or the dashboard stack
  - changing unrelated Linear workflows

## 3. Functional Requirements

1. The CLI must parse an additive provider registry from install-scoped config while preserving the current `agents.default_*` and repo-local `agent.*` fields as a compatibility input.
2. The runtime must resolve an effective provider, model, reasoning level, approval profile, and budget profile from CLI overrides, repo metadata, install-scoped config, and built-in defaults using one deterministic precedence chain.
3. The CLI must expose provider inspection commands that list configured providers, report capabilities and model metadata, distinguish static diagnostics from active probes, and support human-readable plus `--json` output.
4. The CLI must expose policy commands that show the effective approval and budget policy for the repository and apply repo-local policy selections without forcing manual JSON edits.
5. The runtime must record the effective provider/model/policy decision and any fallback activation for agent-backed runs launched from backlog, workflow, scan, technical, plan, and listen flows.
6. The CLI must persist token and cost evidence in a machine-readable local format and use that same data source for `meta budgets status` output.
7. Existing single-agent repositories must continue to launch successfully without adding new provider-registry blocks.
8. Errors for unsupported model selection, missing provider executables, failed provider probes, invalid profile names, and budget enforcement must be actionable and test-covered.

## 4. Non-Functional Requirements

- Performance: provider resolution and budget rollups must remain local and fast enough for synchronous CLI use; status commands should not require network access for historical data.
- Reliability: active provider probes must be opt-in; static inspection and policy/status rendering should succeed offline when local config and ledger files are present.
- Security: the CLI must not log secrets in provider diagnostics or budget/status output; approval and budget decisions should reference profile names and sources, not raw credentials.
- Observability: every launched run should emit enough local metadata to explain provider choice, fallback, approval posture, token usage, and estimated cost after the fact.

## 5. Contracts and Interfaces

### 5.1 Inputs

- Install-scoped config in the existing config file loaded by `AppConfig::load()`.
- Repo-local `.metastack/meta.json` loaded by `PlanningMeta::load()`.
- CLI flags on the new `meta providers`, `meta policy`, and `meta budgets` command families plus existing agent-backed commands.
- Runtime usage inputs already available from session execution paths, such as provider/model choice, token counts, and launch outcome.

Validation rules:

- Provider names must normalize through the same lowercase naming rules already used by `normalize_agent_name`.
- Model selection must remain provider-aware and reject unsupported combinations with an actionable message.
- Routing rules must validate that referenced provider names and fallback targets exist.
- Approval and budget profile references must validate against configured profile sets before a run launches or before `meta policy apply` writes repo metadata.
- Budget status must tolerate an empty ledger and render a clear zero-usage state rather than failing.

### 5.2 Outputs

- Human-readable command output for operator inspection.
- Pretty-printed JSON output for `meta providers`, `meta policy`, and `meta budgets` when `--json` is passed.
- Repo-local metadata updates in `.metastack/meta.json` for policy application and any selected provider/profile overrides that are intended to persist.
- Append-only runtime event records under `.metastack/agents/sessions/`, preferably as JSONL so each event is independently parseable.

Expected output shape:

- Provider inspection should report provider name, source, command definition or builtin preset, supported models, health/diagnostic status, and capability hints such as reasoning, MCP/tool support, or approval-mode support when known.
- Policy inspection should report effective approval profile, budget profile, provider/model selection inputs, source precedence, and any unresolved compatibility shims in effect.
- Budget status should report the budget profile, time window, current token totals, estimated cost totals, threshold state, and the ledger path used.
- Runtime event records should include at minimum timestamp, command/run type, provider, model, approval profile, budget profile, token counts when known, estimated cost when known, and fallback or failure metadata when applicable.

Error shape:

- Errors should name the failing provider/profile/model and the config source or command that triggered the failure.
- Budget enforcement errors should state whether the run was blocked before launch or only marked as over-budget after completion.
- Active provider probe failures should report whether the executable was missing, the command failed, or the response could not be interpreted.

### 5.3 Compatibility

- Backward-compat constraints:
  - existing `agents.default_agent`, `agents.default_model`, `agents.default_reasoning`, and custom `agents.commands` entries must keep working
  - existing repo-local `agent.provider`, `agent.model`, and `agent.reasoning` fields must continue to resolve valid launches
  - existing command aliases such as `meta plan`, `meta technical`, and `meta scan` must continue to honor repo and install defaults
- Migration plan:
  - first implementation reads both legacy and new config shapes
  - docs provide explicit examples for both the legacy single-agent path and the new multi-provider path
  - future cleanup of legacy fields is out of scope for this backlog item

## 6. Architecture and Data Flow

- High-level flow:
  1. Load install-scoped config and repo-local planning metadata.
  2. Normalize legacy and new provider/policy inputs into one runtime resolution structure.
  3. Resolve the effective provider/model/approval/budget choice for the command being executed.
  4. Render inspection/status commands directly from the resolved config and event ledger.
  5. For agent-backed launches, execute through one shared runtime resolver and provider adapter layer.
  6. Append runtime usage/cost events and any fallback metadata to the sessions ledger.
  7. Reuse the ledger for budget status and dashboard-oriented rollups.
- Key components:
  - `src/config.rs` for schema, validation, normalization, and precedence
  - `src/cli.rs` and `src/lib.rs` for command parsing and dispatch
  - shared runtime resolution module for provider/model/policy selection
  - agent execution paths in `src/agents/execution.rs`, `src/workflows.rs`, `src/plan.rs`, `src/technical.rs`, and `src/listen/`
  - `src/fs.rs` for new ledger and metadata paths
- Boundaries:
  - no remote state store
  - no external billing integration
  - no orchestration outside this repository root

## 7. Acceptance Criteria

- [ ] Install-scoped config supports multiple providers, model metadata, routing rules, approval profiles, and budget profiles without breaking current single-agent setups.
- [ ] `meta providers list --root .` and `meta providers list --root . --json` show the effective provider registry for this repository with source and capability details.
- [ ] `meta providers doctor --root .` performs static validation without requiring network access, and `meta providers test <PROVIDER> --root .` performs an explicit active probe with actionable failures.
- [ ] `meta policy show --root .` and `meta policy show --root . --json` report the effective approval and budget policy plus where each value came from.
- [ ] `meta policy apply --root .` updates `.metastack/meta.json` in a deterministic, validation-backed way for repo-local policy selection.
- [ ] `meta budgets status --root .` and `meta budgets status --root . --json` read the persisted event ledger and report token/cost totals against the selected budget profile.
- [ ] Agent-backed runs record effective provider, model, approval profile, budget profile, and fallback decisions in local runtime evidence under `.metastack/agents/sessions/`.
- [ ] README and workflow docs describe the new config contract, operator commands, and compatibility path.
- [ ] `cargo test` and focused command proofs cover success and failure cases for provider inspection, policy application, budget status, and legacy single-agent compatibility.

## 8. Test Plan

- Unit tests:
  - config parsing and validation for legacy plus new registry shapes
  - precedence and normalization logic for provider/model/policy resolution
  - budget rollup logic over append-only event records
- Integration tests:
  - command parsing and rendering for `meta providers`, `meta policy`, and `meta budgets`
  - runtime launch tests showing resolved provider/model/policy metadata is emitted for workflow, plan, technical, scan, and listen paths where applicable
- Contract tests:
  - legacy single-agent fixtures still resolve the same provider/model behavior
  - JSON output remains stable enough for dashboards and downstream tooling
- Negative-path tests:
  - missing provider executable
  - unsupported provider/model pairing
  - invalid routing target or profile name
  - failed provider probe
  - empty ledger and over-budget ledger states

## 9. Open Questions

1. Should budget enforcement ever block a run before launch in this first iteration, or should the first version be read-only status plus warnings?
2. How much capability metadata can be inferred locally versus declared in config for custom providers?
3. Does `meta policy apply` need separate flags for approval and budget profile selection, or should it apply a named combined policy bundle?

## 10. Linked Workstreams

- Registry and compatibility groundwork: [`./tasks/workstream-template.md`](./tasks/workstream-template.md)

# Checklist: Technical: Technical: Add multi-provider registry, approval profiles, and budget governance for fleet operations

Last updated: 2026-03-16

## 1. Baseline and Decisions

- [ ] Confirm the repository-scoped problem statement and non-goals in [`index.md`](./index.md).
- [ ] Lock the additive config contract, precedence rules, and persistence plan in [`specification.md`](./specification.md).
- [ ] Decide whether `meta providers`, `meta policy`, and `meta budgets` ship as new top-level command families or compatibility aliases to another namespace.
- [ ] Confirm the append-only event ledger path and JSON shape before implementation work starts.
- [ ] Confirm owners and review expectations in [`contacts.md`](./contacts.md).

## 2. Implementation Tasks by Area

### Area: Config and Compatibility

- [ ] Extend install-scoped config in `src/config.rs` to model providers, model metadata, routing rules, approval profiles, and budget profiles.
- [ ] Extend repo-scoped `.metastack/meta.json` handling to persist effective provider/policy selections without breaking current `agent.provider`, `agent.model`, and `agent.reasoning` behavior.
- [ ] Define and test precedence for CLI overrides, repo metadata, install-scoped config, built-in defaults, and environment fallbacks.
- [ ] Add compatibility coverage proving existing single-agent setups still resolve valid launches.

### Area: Operator Surfaces

- [ ] Wire new command parsing and dispatch in `src/cli.rs` and `src/lib.rs`.
- [ ] Implement `meta providers list|test|doctor` with human-readable and `--json` output.
- [ ] Implement `meta policy show|apply` with clear source-of-truth reporting and repo-local write behavior for `apply`.
- [ ] Implement `meta budgets status` over the persisted event ledger with budget threshold reporting.

### Area: Runtime Routing and Persistence

- [ ] Add a shared provider-resolution layer used by backlog, workflow, scan, and listen execution paths.
- [ ] Record effective provider, model, routing decision, approval profile, and budget profile for each launched run.
- [ ] Persist usage, token, and cost events in an append-only local format under `.metastack/agents/sessions/`.
- [ ] Ensure fallback behavior is explicit, deterministic, and inspectable when a provider or model cannot be used.

### Area: Docs and Reviewability

- [ ] Update [`../../../README.md`](../../../README.md) with config examples and operator commands.
- [ ] Update [`../../../WORKFLOW.md`](../../../WORKFLOW.md) where workflow or operator guidance changes.
- [ ] Add or update tests in `tests/config.rs`, `tests/commands.rs`, `tests/workflows.rs`, `tests/listen.rs`, and any new module-level unit coverage required by the implementation.
- [ ] Make sure the backlog packet links and examples stay consistent with the final CLI names and filesystem paths.

## 3. Cross-Cutting Quality Gates

- [ ] Deterministic provider resolution is verified for identical config and repo metadata.
- [ ] Static diagnostics and active provider tests are clearly separated so `doctor` remains usable without flaky network dependencies.
- [ ] Error messages explain whether failures came from config validation, provider health, unsupported model selection, or budget enforcement.
- [ ] The event ledger can be consumed by status commands without parsing raw log text.
- [ ] No existing repo command loses current behavior unless the change is explicitly documented and covered by tests.

## 4. Exit Criteria

- [ ] Every `Definition of Done` item in [`index.md`](./index.md) is complete.
- [ ] The planned PR slices in [`proposed-prs.md`](./proposed-prs.md) are either merged as scoped slices or intentionally collapsed with rationale.
- [ ] Remaining risks in [`risks.md`](./risks.md) have an owner and explicit mitigation or acceptance note.
- [ ] [`validation.md`](./validation.md) has concrete passing command proofs and filesystem evidence.

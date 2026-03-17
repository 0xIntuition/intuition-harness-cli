# Unify built-in agent provider resolution, adapter architecture, and reasoning selection for Codex and Claude

## Problem
Users can configure `claude` but still suspect `codex` is being launched. Provider resolution is currently spread across config, route helpers, workflow launching, cron setup, listen/scan flows, and execution paths, while reasoning is still configured as free text even though supported levels depend on the selected provider/model. This creates ambiguity in provider selection, duplicated built-in provider logic, weak runtime explainability, and invalid reasoning combinations that are easy to save.

## Scope
Deliver one repository-wide redesign of agent configuration and launch behavior for the built-in `codex` and `claude` providers. Introduce a shared provider adapter interface that becomes the single entry point for provider-specific metadata and launch behavior, migrate current built-in providers behind it, replace free-text reasoning with provider/model-specific selectable options from a static in-repo catalog, and redefine precedence across install-scoped config, repo-scoped `.metastack/meta.json`, route overrides, and explicit CLI overrides so the new configuration rules win consistently.

This work should audit every agent-backed command path in this repository, including normal execution, workflow runs, listen workers, scan/reload flows, cron setup, merge helpers, and any other shared launch path. Add runtime diagnostics or dry-run visibility that expose the resolved provider, model, reasoning, route key, and config source before launch so misrouting can be proven quickly. Keep scope limited to this repository and to the built-in `codex`/`claude` providers, but shape the adapter so future providers can be added cleanly.

## Sequencing
1. Reproduce and fix incorrect effective provider selection, then add runtime/provider-resolution diagnostics.
2. Introduce the built-in provider adapter layer and move `codex` and `claude` launch/config metadata behind it while preserving workspace safety safeguards.
3. Replace free-text reasoning with selectable provider/model-specific options across setup and runtime config flows and validate persisted config values.
4. Redefine and document one precedence model for provider/model/reasoning resolution, removing legacy fallback ambiguity even if this is breaking.
5. Add integration coverage, command-path proofs, CLI help, and documentation updates for the new behavior.

## Notes
Breaking config changes are acceptable. Do not add arbitrary custom providers yet, but ensure provider metadata used by config/setup/runtime flows comes from the adapter layer instead of duplicated hard-coded lists.

## Acceptance Criteria

- When repo or install config selects `claude`, agent-backed commands in this repository launch `claude` rather than silently falling back to `codex` unless an explicit override says otherwise.
- A shared provider adapter interface exists for built-in agent providers and is the single entry point for provider-specific launch behavior and metadata in this repository.
- The existing built-in `codex` and `claude` implementations are migrated behind the adapter layer without changing unrelated command behavior, and Codex-specific workspace safety/sandbox safeguards remain enforced through the adapter path.
- Runtime output, dry-run output, or diagnostics expose the resolved provider, model, reasoning, route key, and the source used to resolve them, including explicit override, route key, repo default, and install/global default.
- Setup and runtime config flows present reasoning as a selectable value tied to the selected provider/model rather than a free-text field.
- Built-in `codex` and `claude` models expose documented supported reasoning options from a static in-repo catalog, invalid provider/model/reasoning combinations are rejected with actionable errors, and persisted install-scoped and repo-scoped config stores only supported values.
- The repository has one documented precedence order for provider/model/reasoning resolution, the implementation matches it consistently across agent-backed commands, and legacy fallback behavior that can override newer configuration unexpectedly is removed or updated.
- Automated tests cover provider resolution across repo defaults, install/global defaults, route overrides, and explicit CLI overrides for built-in `codex` and `claude` paths, including reasoning-option refresh and invalid-value rejection.
- Integration tests cover setup/config selection, persisted config shape, and at least one end-to-end launch proof for both built-in providers.
- CLI help text and repository documentation are updated to explain supported `codex`/`claude` provider-model-reasoning combinations, the new precedence rules, and the command/diagnostic path for confirming effective provider selection before or during launch.
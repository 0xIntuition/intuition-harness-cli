# Backlog Item: Technical: Technical: Add multi-provider registry, approval profiles, and budget governance for fleet operations

This backlog item expands the `metastack-cli` runtime from a single resolved local agent into an inspectable fleet runtime for this repository only. The work is scoped to the Rust CLI rooted at this backlog item's repository and builds on the existing config loader in `src/config.rs`, agent launch path in `src/agents/execution.rs`, repo metadata in `.metastack/meta.json`, session persistence under `.metastack/agents/sessions/`, and command-level coverage in `tests/`.

## Required Files

- `index.md`: summary of the technical slice, scope, milestones, and repo-level definition of done.
- `specification.md`: contract for provider registry, approval policy, budget governance, persistence, and command surfaces.
- `checklist.md`: execution checklist organized around config, runtime routing, operator surfaces, telemetry, tests, and docs.
- `contacts.md`: role-based owners, reviewers, and escalation points for CLI runtime governance work.
- `proposed-prs.md`: review-sized PR slices mapped to the real source and test areas in this crate.
- `decisions.md`: decision log for command placement, compatibility behavior, and persistence format.
- `risks.md`: risk register and open design questions that should be closed before implementation expands scope.
- `implementation.md`: expected workstreams, touchpoints, and validation strategy across command handlers, config, runtime, and docs.
- `validation.md`: command proofs and filesystem evidence expected before the item is considered complete.

## Supporting Folders

- `context/`: local research notes about current config precedence, agent invocation, session telemetry, and command ergonomics.
- `tasks/`: workstream-level planning notes for registry/config, policy surfaces, and budgets/telemetry.
- `artifacts/`: captured evidence such as schema drafts, JSON output snapshots, and compatibility proofs.

## Repository Scope

- Repository: `metastack-cli`
- Root crate: `metastack-cli` in `Cargo.toml`
- Primary code areas: `src/cli.rs`, `src/lib.rs`, `src/config.rs`, `src/config_command.rs`, `src/agents/`, `src/workflows.rs`, `src/listen/`, `src/fs.rs`, `README.md`, `WORKFLOW.md`, and `tests/`
- Existing repo-local state: `.metastack/meta.json`, `.metastack/backlog/`, `.metastack/codebase/`, `.metastack/agents/sessions/`

## Planning Notes

1. Preserve the current single-agent setup path as a compatibility layer; do not require manual migration for repos already using `agent.provider`, `agent.model`, or install-scoped `agents.default_*` settings.
2. Keep operator-facing governance surfaces explicit and inspectable. The backlog assumes dedicated `meta providers`, `meta policy`, and `meta budgets` command families rather than burying these concerns in ad hoc config mutations.
3. Treat `.metastack/agents/sessions/` as the natural home for append-only run evidence because it already stores scan logs and listen session state for repo-scoped runtime behavior.
4. Keep docs and validation in scope. This item is not complete without concrete command proofs, JSON output proofs, and updated contributor-facing docs.

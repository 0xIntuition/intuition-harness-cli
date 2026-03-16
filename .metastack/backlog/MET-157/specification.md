# Specification: Technical: Add issue readiness scoring and safe multi-ticket kickoff for small teams

Version: 0.1  
Last updated: 2026-03-16

Parent index: [`./index.md`](./index.md)

## 1. Executive Summary

Deliver a shared readiness layer for `metastack-cli` that scores Todo issues before unattended execution, explains why tickets are not yet eligible, and lets small teams safely launch a bounded batch of ready workspaces through the existing listen runtime. The main users are operators running `meta agents listen` and maintainers reviewing current queue quality through session and dashboard surfaces.

## 2. Problem Statement

- Problem: `meta agents listen` can already claim Todo work, but it primarily relies on binary eligibility checks and per-cycle pickup count. Small teams still need manual triage to understand which tickets are actually ready, why others were skipped, and when concurrency would become unsafe.
- Why now: parent issue `MET-145` requires readiness scoring, dependency filtering, missing-context detection, bounded multi-ticket kickoff, and visible session state without weakening workspace safety.
- Non-goals:
  - redesign the full listen dashboard information architecture
  - implement multi-host coordination beyond the current per-project local lock
  - refactor all of `src/listen/mod.rs` as part of this ticket

## 3. Functional Requirements

1. The CLI must evaluate candidate Todo issues and classify each as `ready`, `blocked`, `missing_context`, or `human_needed`.
2. The evaluator must produce a numeric readiness score plus human-readable downgrade reasons and a next-action hint.
3. Readiness inputs must combine current Linear issue metadata with repo-local signals, including at minimum:
   - issue workflow state
   - required listen label and assignee-scope filters
   - active session state from the persisted listen project store
   - issue description and acceptance/validation completeness
   - dependency-related signals required to block unsafe kickoff
   - presence of repo-local setup and codebase context needed by unattended runs
4. A read-only command surface must render readiness results without mutating Linear state or creating workspaces.
5. `meta agents listen` must reuse the same evaluator when deciding which Todo issues to claim.
6. Listen pickup must respect a configurable active-session concurrency limit in addition to the existing per-cycle pickup count.
7. Tickets excluded from pickup must retain an explanation that is visible in current queue or session surfaces.
8. Batch kickoff must continue to provision isolated sibling workspaces only under the configured workspace root and must never run implementation turns in the source repository.
9. Session persistence must remain visible through existing inspect and dashboard surfaces after readiness-driven pickup occurs.

## 4. Non-Functional Requirements

- Performance: one readiness pass for the configured issue limit should fit within the current poll cadence and avoid redundant Linear round-trips when existing issue data is sufficient.
- Reliability: readiness evaluation must be deterministic for identical issue data and config; concurrency gating must not produce duplicate active sessions for the same issue.
- Security: new logic must not weaken workspace-root checks, source-repo protection, or current secret-handling boundaries in listen workers.
- Observability: CLI output, notes, and persisted state must make it obvious why a ticket was skipped, downgraded, or launched.

## 5. Contracts and Interfaces

### 5.1 Inputs

- Read-only evaluation command:
  - `meta agents readiness --root <PATH> --team <TEAM> --project <PROJECT> --limit <N> [--json]`
- Execution command:
  - `meta agents listen --root <PATH> --team <TEAM> --project <PROJECT> --once --max-pickups <N> --max-concurrency <N>`
- Repo config:
  - add an optional `listen.max_concurrency` setting under `.metastack/meta.json`
- Runtime data:
  - current listen project `session.json`
  - current repo-local `.metastack/` setup and codebase context files

Validation rules:

- `max_concurrency` must be an integer greater than or equal to `1`.
- The readiness command must fail with the existing repo-setup guidance when `.metastack/meta.json` is missing.
- The readiness command must not create or mutate backlog files, workspaces, Linear state, or the install-scoped listen store.
- When `max_concurrency` is omitted, the default must preserve current single-session-safe behavior.

### 5.2 Outputs

- Readiness result record:
  - `identifier`
  - `title`
  - `classification`
  - `score`
  - `reasons[]`
  - `next_action`
  - `would_pick_up`
- Text output:
  - concise table or summary grouped by classification with per-ticket reasons
- JSON output:
  - machine-readable list of readiness records plus summary counts
- Persisted state:
  - active session entries remain in the existing listen store and include enough summary text or metadata to explain readiness-driven kickoff outcomes

Error shape:

- missing repo setup, invalid config, or unsupported flags return the existing CLI error style with actionable text
- dependency or context gaps do not fail the whole readiness command; they classify the affected tickets instead
- unsafe workspace or state conflicts still fail kickoff with explicit error messages and no partial source-repo writes

### 5.3 Compatibility

- Backward-compat constraints:
  - current `meta agents listen` behavior remains compatible when the new concurrency limit is not set beyond the default
  - legacy `meta listen` alias continues to work for execution and session inspection
  - existing session-inspection flows must continue to render even if readiness metadata is absent in older state files
- Migration plan:
  - add the config field as optional
  - treat missing readiness metadata in persisted state as unknown/legacy and render gracefully
  - document the new command and config path in `README.md` and agent-daemon docs

## 6. Architecture and Data Flow

- High-level flow:
  1. Load repo root, repo config, and current install-scoped listen project store.
  2. Fetch candidate Todo issues using existing Linear service filters.
  3. Evaluate each candidate with the shared readiness layer.
  4. Render readiness output for the read-only command, or feed the `ready` subset into listen pickup.
  5. Before claiming work, compare active non-completed sessions against `max_concurrency`.
  6. For eligible tickets, reuse current backlog/workpad/workspace provisioning and worker launch flow.
  7. Persist updated session state and expose summaries through inspect and dashboard views.
- Key components:
  - CLI and dispatch: `src/cli.rs`, `src/lib.rs`
  - Config and setup: `src/config.rs`, `src/setup.rs`
  - Readiness + pickup flow: `src/listen/mod.rs`, `src/listen/state.rs`, `src/listen/store.rs`
  - Visibility: `src/listen/dashboard.rs`, `src/listen/web.rs`
  - Safety: `src/listen/workspace.rs`
  - Linear inputs: `src/linear/service.rs`, `src/linear/transport.rs`, `src/linear/types.rs`
- Boundaries:
  - no changes outside this repository
  - no direct writes in the source checkout during unattended execution
  - no new persistence system outside the current listen project store

## 7. Acceptance Criteria

- [ ] The CLI can classify candidate issues as ready, blocked, missing context, or human needed using Linear metadata and repo-local context signals.
- [ ] `meta agents listen` can launch a bounded batch of ready tickets into isolated workspaces while respecting a configurable concurrency limit.
- [ ] Readiness output explains why a ticket was excluded or downgraded and points to the next improvement step.
- [ ] Session state for readiness-driven kickoff is persisted and visible through existing listen/session surfaces.
- [ ] Tests cover dependency filtering, missing-context detection, concurrency limits, and workspace safety guarantees.
- [ ] Default behavior remains backward-compatible for current single-ticket listen users.

## 8. Test Plan

- Unit tests:
  - readiness score and class calculation
  - hard-block vs downgrade-only reason handling
  - active-session concurrency counting
- Integration tests:
  - read-only readiness command output and JSON mode
  - `meta agents listen --once` pickup with multiple ready candidates and a concurrency limit
  - session inspect output after bounded kickoff
- Contract tests:
  - config parsing and validation for the new concurrency setting
  - persisted state decoding when readiness metadata is absent or present
- Negative-path tests:
  - missing repo setup
  - dependency-blocked ticket excluded from pickup
  - missing-context ticket reported but not launched
  - unsafe workspace state or conflict prevents pickup without touching the source repo

## 9. Open Questions

1. Does the first implementation need full blocked-by relation support from Linear, or can it satisfy the dependency gate with existing issue relationships plus a minimal transport addition?
2. Should the readiness command live only under `meta agents`, or should a compatibility alias be added in the same release?
3. How much readiness detail belongs in the compact dashboard queue versus the richer inspect and JSON views?

## 10. Linked Workstreams

- Workstream A: [`./tasks/workstream-template.md`](./tasks/workstream-template.md)

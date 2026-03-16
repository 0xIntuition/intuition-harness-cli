# Specification: Technical: Implement the end-to-end `meta merge` command, dashboard, agent merge-run orchestration, and aggregate PR publication

Version: 0.1  
Last updated: 2026-03-16

Parent index: [`./index.md`](./index.md)

## 1. Executive Summary

Deliver a repository-scoped `meta merge` workflow for `metastack-cli` that discovers open GitHub pull requests targeting `main`, presents them in a one-shot ratatui dashboard, captures a local merge plan through the configured agent, executes the selected batch in a safe workspace outside the source checkout, reruns repository validation, and publishes one aggregate PR back to `main` with deterministic local run artifacts under `.metastack/merge-runs/<RUN_ID>/`.

## 2. Problem Statement

- Problem: the CLI can plan work, sync backlog files, run agents, and supervise unattended issue execution, but it has no repository-scoped workflow for batching multiple open GitHub pull requests into a single reviewed aggregate merge.
- Why now: parent issue `MET-161` defines `meta merge` as a first-class feature and requires CLI help, dashboard behavior, agent orchestration, artifact persistence, and GitHub publication to work as one coherent repo-local workflow.
- Non-goals:
  - replacing GitHub's hosted merge queue or branch protection system
  - creating a background listener or persisted daemon for merge batching
  - supporting cross-repository batching or arbitrary base branches in the first slice
  - introducing live GitHub API dependencies in tests

## 3. Functional Requirements

1. Add a top-level `meta merge` command family to the public CLI surface in `src/cli.rs` and dispatch it from `src/lib.rs`.
2. Resolve the active GitHub repository from the current checkout through `gh` and fail early with clear remediation when `gh` is unavailable, unauthenticated, or cannot resolve the repo.
3. List open pull requests targeting `main`, load the metadata needed by the dashboard and planner, and expose that same data through a deterministic non-interactive proof path such as `meta merge --json`.
4. When stdout is attached to a TTY, open a one-shot ratatui dashboard that supports multi-selection, explicit cancellation, and a launch summary that makes the one-shot execution model clear.
5. Provide a deterministic dashboard snapshot path such as `meta merge --render-once` with scripted events so layout and interaction states can be tested in CI.
6. Before execution, create a fresh aggregate workspace outside the source checkout from the latest `origin/main` and persist a stable run directory under `.metastack/merge-runs/<RUN_ID>/`.
7. Capture selected PR metadata, repo context, and agent-produced planning output in the run directory before merge execution begins.
8. Invoke the configured local agent with enough repository and PR context to choose an explicit merge order and identify likely conflict hotspots.
9. Apply the selected PRs in explicit order onto an aggregate branch, record per-PR outcomes locally, and invoke the agent again when conflict resolution or integration repair is required.
10. Rerun repository validation commands after local merge completion and refuse aggregate PR publication when validation fails.
11. Push the aggregate branch and create or update one GitHub PR back into `main` whose title and body identify the included source PRs.
12. Update repository docs and prompts so usage, prerequisites, artifact layout, and validation expectations are explicit and maintainable.

## 4. Non-Functional Requirements

- Performance: PR discovery and dashboard startup should stay fast enough for interactive use on typical repositories; no live-network calls should be required in tests.
- Reliability: merge execution must be restartable as a new run without corrupting prior artifacts; failed runs must leave a concrete blocker report and preserved evidence.
- Security: execution must never run in the source checkout; the feature must rely on the same trusted-local-agent model already used elsewhere in the repo and should not leak auth details into artifacts.
- Observability: local artifacts and user-facing output must make the active repo, selected PRs, merge order, validation result, and publication result easy to audit.

## 5. Contracts and Interfaces

### 5.1 Inputs

- Active repository root, defaulting to the current checkout.
- A git checkout with `origin/main` available locally or fetchable.
- `gh` on `PATH` with auth sufficient for repo inspection and PR create or update operations.
- Repo-local `.metastack/` state and any required repo defaults from `.metastack/meta.json`.
- Configured local agent defaults from install-scoped config, with optional command-line overrides if added in scope.
- Deterministic test inputs for `--json`, `--render-once`, scripted dashboard events, stubbed `gh`, and temp git repositories.

Validation rules:

- Fail when the root is not a git checkout or the active repo cannot be resolved.
- Fail when `gh` is missing or `gh auth status` or equivalent repo-resolution commands fail.
- Fail when no open PRs are available for selection or when launch is attempted with an empty selection.
- Fail when the computed workspace path resolves to the source checkout or outside the allowed workspace root.
- Fail publication when validation commands do not pass.

### 5.2 Outputs

- User-facing stdout or stderr for help text, JSON metadata, dashboard snapshots, batch summaries, validation status, and publish results.
- Deterministic local run artifacts under `.metastack/merge-runs/<RUN_ID>/`, at minimum including:
  - `repo.json`
  - `pull-requests.json`
  - `selection.json`
  - `merge-plan.md`
  - `status.json`
  - `validation.json`
  - `aggregate-pr.md`
  - `blocker.md` on failure
- Git side effects confined to the isolated aggregate workspace and the pushed aggregate branch.
- GitHub side effects limited to creating or updating the aggregate PR back into `main`.

Error shape:

- User-facing failures should identify the failing stage and the failing tool or prerequisite, for example repo resolution, `gh` auth, workspace creation, merge application, validation, or aggregate PR publication.
- Run artifacts should preserve enough context for a reviewer to understand what failed without replaying the entire run.

### 5.3 Compatibility

- Backward-compat constraints: the new `meta merge` surface must be additive and must not change the behavior of existing command families such as `meta backlog`, `meta dashboard`, or `meta agents listen`.
- Migration plan: follow existing repo patterns for deterministic non-interactive flags and dashboard snapshots so the new command fits the current CLI design language without requiring a separate operator model.

## 6. Architecture and Data Flow

- High-level flow:
  1. Parse `meta merge` args and resolve the active repository root.
  2. Call the `gh` adapter to resolve repo identity and list open PR metadata.
  3. Return JSON immediately for `--json`, render a snapshot for `--render-once`, or open the TTY dashboard otherwise.
  4. On launch, create a fresh aggregate workspace from `origin/main` outside the source checkout.
  5. Persist initial merge-run artifacts under `.metastack/merge-runs/<RUN_ID>/`.
  6. Build the merge-planning prompt with repo scope, selected PR metadata, and conflict hints, then invoke the configured local agent.
  7. Apply the selected PRs in explicit order, invoking the agent again when conflicts require resolution.
  8. Rerun repository validation and refuse publication on failure.
  9. Push or update the aggregate branch and create or update the aggregate GitHub PR.
  10. Persist final run status and publication summary.
- Key components:
  - `src/cli.rs` and `src/lib.rs` for command parsing and dispatch
  - a new merge implementation module for GitHub transport, dashboard state, runner, and artifact serialization
  - `src/fs.rs` for `.metastack/merge-runs` path helpers
  - `src/listen/workspace.rs` or extracted shared helpers for safe workspace provisioning
  - `src/agents/execution.rs` for subprocess-based planning and conflict-resolution agent calls
  - `README.md`, `WORKFLOW.md`, and prompt files for documentation and operator guidance
- Boundaries:
  - GitHub access is mediated through `gh`, not a new network SDK.
  - Agent execution remains subprocess-based and repo-local.
  - Tests must stub `gh` and use temp repositories instead of live GitHub data.

## 7. Acceptance Criteria

- [ ] `meta merge --help` documents the new command family and its one-shot dashboard behavior.
- [ ] The implementation discovers the current GitHub repository from the active checkout and fails with a clear error when `gh` auth or repo resolution is unavailable.
- [ ] A deterministic non-interactive proof path exists for tests, such as `meta merge --json` or equivalent, that emits the open PR metadata needed by the dashboard and planner.
- [ ] Integration tests stub `gh` and verify PR discovery, repo resolution, and error handling without calling the live GitHub API.
- [ ] Running `meta merge` in a TTY opens a dashboard that lists open PRs from the current repo and allows selecting multiple entries before launch.
- [ ] The dashboard includes a deterministic snapshot path such as `meta merge --render-once` with scripted events so layout and interaction states can be tested.
- [ ] The selected batch summary clearly shows which PRs will be handed to the merge agent and that the flow is one-shot rather than a persisted listener session.
- [ ] Tests cover empty-state, single-selection, multi-selection, and cancellation behavior.
- [ ] Launching a merge batch creates a fresh workspace rooted outside the source checkout and based on the latest `origin/main` for the current repository.
- [ ] Each run writes deterministic local artifacts under `.metastack/merge-runs/<RUN_ID>/` that include the selected PR set, relevant GitHub context, and the agent-produced merge plan.
- [ ] The planning prompt includes enough repo and PR context for the agent to choose an explicit merge order and call out likely conflict hotspots before execution.
- [ ] Tests verify the workspace safety rules, run artifact layout, and repeat-run behavior for multiple merge batches.
- [ ] The merge runner applies the selected PRs onto the aggregate branch in an explicit order and records per-PR progress and outcomes in the local run artifacts.
- [ ] When a merge conflict occurs, the command invokes the configured local agent in the aggregate workspace, captures the resolution steps, and either continues successfully or stops with a concrete blocker report.
- [ ] After the batch is merged locally, the command reruns the configured validation commands for the repo and refuses to publish the aggregate PR when validation fails.
- [ ] A successful run pushes the aggregate branch and opens or updates a GitHub PR back into `main` whose title and body identify the batched PRs included in the merge.
- [ ] Integration tests cover both a clean multi-PR batch and a conflict-requiring batch using stubbed `gh`, temp git repos, and deterministic validation commands.
- [ ] `README.md` documents how to run `meta merge`, what the dashboard does, the required `gh` setup, and what outputs are created locally and on GitHub.
- [ ] Any behavior or workflow-contract changes needed for agent-run merge orchestration are reflected in the relevant repo docs and prompts in this repository.
- [ ] The test suite includes an end-to-end proof that exercises command entry, dashboard launch or snapshot behavior, merge-run artifact creation, and aggregate PR publication through stubs.
- [ ] The feature's validation path is explicit and reproducible with repository-local commands suitable for CI and manual verification.

## 8. Test Plan

- Unit tests:
  - parse `gh` output into stable repo and PR structs
  - render artifact manifests and aggregate PR body summaries deterministically
  - enforce workspace path and branch-name safety helpers
- Integration tests:
  - stub `gh` on `PATH` and verify repo resolution, PR discovery, and publication commands
  - use temp git repos to prove clean multi-PR batches and conflict-driven batches
  - snapshot dashboard states through `--render-once` and scripted events
- Contract tests:
  - assert the `--json` output shape consumed by the dashboard and planner
  - assert the `.metastack/merge-runs/<RUN_ID>/` file layout and required fields
- Negative-path tests:
  - missing `gh`
  - unauthenticated `gh`
  - repo-resolution failure
  - empty PR list
  - empty selection launch attempt
  - validation failure after merge
  - unresolved conflict after agent invocation
  - aggregate PR publication failure

## 9. Open Questions

1. Should merge validation default to `make all` or allow a merge-specific repo-configured command list from day one?
2. Should run artifacts keep full agent transcripts, or only summarized prompts and outcomes plus separate logs on demand?
3. Should aggregate PR reuse prefer a stable branch name derived from the selection, or always create a fresh run-specific branch?

## 10. Linked Workstreams

- Initial workstream: [`./tasks/workstream-template.md`](./tasks/workstream-template.md)
- Validation and proof checklist: [`./validation.md`](./validation.md)

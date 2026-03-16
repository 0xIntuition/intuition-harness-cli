# Technical: Implement the end-to-end `meta merge` command, dashboard, agent merge-run orchestration, and aggregate PR publication

Last updated: 2026-03-16

## Parent Issue

- Identifier: `MET-161`
- Title: Implement the end-to-end `meta merge` command, dashboard, agent merge-run orchestration, and aggregate PR publication
- URL: https://linear.app/metastack-backlog/issue/MET-161/implement-the-end-to-end-meta-merge-command-dashboard-agent-merge-run

## Repository Scope

- Root: `/Users/metasudo/workspace/metastack/stack/metastack-cli`
- Default scope: the full repository rooted above
- In scope: CLI command wiring, GitHub transport, ratatui dashboard behavior, merge-run workspace and artifact handling, agent prompts, documentation, and tests inside this repository
- Out of scope: sibling repositories, hosted merge-queue services, and background daemon behavior for `meta merge`

## Context

`metastack-cli` already has strong patterns for clap-driven command families, deterministic `--json` and `--render-once` proof paths, subprocess-based local agents, and safe workspace clones for unattended work. What it does not yet have is a repository-scoped merge batching workflow that can inspect open GitHub pull requests, let an engineer select multiple PRs in one ratatui session, ask the configured local agent to propose a merge order and conflict hotspots, execute the batch in a safe workspace outside the source checkout, and publish one aggregate PR back to `main` with a local audit trail under `.metastack/merge-runs/`.

## Proposed Approach

1. Add a new top-level `meta merge` command family in `src/cli.rs` and `src/lib.rs`.
2. Implement a focused `gh` adapter for repository resolution, open-PR discovery, PR detail inspection, and aggregate PR create or update behavior.
3. Reuse the repository's existing deterministic dashboard conventions by providing both `meta merge --json` and `meta merge --render-once` proof paths.
4. Create a safe merge runner that provisions an isolated workspace, persists deterministic run artifacts, invokes the configured local agent for planning and conflict help, reruns repository validation, and blocks publication on failure.
5. Update repo-local docs and prompts so the `gh` prerequisite, one-shot dashboard semantics, merge-run artifact layout, and validation path are explicit.

## Milestones

1. Command surface and `gh` transport contract are stable.
2. Dashboard selection flow and snapshot coverage are stable.
3. Merge-run planning, execution, and publication are stable in hermetic temp-repo tests.
4. Documentation and validation commands are complete.

## Risks

- `gh` is an external prerequisite and will fail on some machines unless preflight checks are explicit.
- Merge execution spans git state, agent prompts, TUI interaction, and GitHub publication; a weak integration harness will miss regressions.
- Workspace safety is non-negotiable because merge execution must never mutate the source checkout.

## Validation

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] Focused tests for PR discovery, dashboard snapshots, merge-run artifacts, and aggregate PR publication
- [ ] End-to-end temp-repo proof for clean and conflict-driven batches
- [ ] `make all`

## Definition of Done

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

## Linked Docs

- Contract: [`./specification.md`](./specification.md)
- Checklist: [`./checklist.md`](./checklist.md)
- PR strategy: [`./proposed-prs.md`](./proposed-prs.md)
- Risks: [`./risks.md`](./risks.md)
- Validation: [`./validation.md`](./validation.md)
- Workstream: [`./tasks/workstream-template.md`](./tasks/workstream-template.md)

## Next Actions

1. Lock the command-line contract and the merge-run artifact schema before starting implementation.
2. Reuse the repository's existing `--render-once` and `TestBackend` patterns for dashboard proofs.
3. Share or extract workspace-safety helpers so the merge runner cannot execute inside the source checkout.
4. Keep the first implementation slices hermetic by stubbing `gh` and using temp git repositories throughout.

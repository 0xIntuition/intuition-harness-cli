# Checklist: Technical: Implement the end-to-end `meta merge` command, dashboard, agent merge-run orchestration, and aggregate PR publication

Last updated: 2026-03-16

## 1. Baseline and Decisions

- [ ] Confirm the child issue stays repository-scoped to `metastack-cli` and does not spill into sibling repos.
- [ ] Confirm the additive `meta merge` command contract in [`index.md`](./index.md) and [`specification.md`](./specification.md).
- [ ] Confirm the `gh` transport, safe workspace model, and merge-run artifact layout in [`decisions.md`](./decisions.md).
- [ ] Confirm role-based ownership and escalation notes in [`contacts.md`](./contacts.md).

## 2. Implementation Tasks by Area

### Area: CLI Surface and GitHub Transport

- [ ] Add a new top-level `meta merge` command family in `src/cli.rs` and dispatch in `src/lib.rs`.
- [ ] Implement a focused `gh` wrapper for repository resolution, open PR discovery, PR detail lookup, and aggregate PR create/update behavior.
- [ ] Surface clear preflight failures for missing `gh`, missing auth, repo-resolution failure, and non-git roots.
- [ ] Add deterministic `--json` output for PR metadata and planning inputs.

### Area: Dashboard and Deterministic UX

- [ ] Implement a ratatui one-shot multi-PR selection dashboard following the repo's `--render-once` pattern.
- [ ] Add a batch summary view that clearly states the selected PRs, merge order handoff, and one-shot execution model.
- [ ] Cover empty-state, single-selection, multi-selection, and cancellation behavior.
- [ ] Add scripted-event snapshot coverage so layout and interaction states stay deterministic in CI.

### Area: Merge Runner, Workspaces, and Agent Orchestration

- [ ] Reuse or extract workspace safety helpers so merge execution never runs inside the source checkout.
- [ ] Extend `.metastack` path handling to persist deterministic merge-run artifacts under `.metastack/merge-runs/<RUN_ID>/`.
- [ ] Build the planning prompt with repo scope, selected PR metadata, and likely conflict hotspots.
- [ ] Apply the selected PRs in explicit order and record per-PR state transitions in local artifacts.
- [ ] Invoke the configured local agent for conflict resolution when needed, then continue or stop with a concrete blocker report.
- [ ] Rerun repository validation commands and block aggregate PR publication when validation fails.

### Area: Docs, Prompts, and Validation Proofs

- [ ] Update `README.md` with `meta merge` usage, `gh` prerequisites, dashboard behavior, and merge-run outputs.
- [ ] Update affected prompt or workflow-contract files in this repository if the merge agent needs additional injected context.
- [ ] Add hermetic tests for `gh` stubbing, temp git repos, publication flow, and repeat-run behavior.
- [ ] Keep command proofs and quality gates current in [`validation.md`](./validation.md).

## 3. Cross-Cutting Quality Gates

- [ ] Deterministic behavior is verified for PR discovery JSON, dashboard snapshots, run artifact manifests, and aggregate PR body generation.
- [ ] No live GitHub API calls are required in tests; all GitHub interactions are stubbed through `gh` and temp repos.
- [ ] Workspace-safety checks explicitly prove the source checkout is never used as the merge execution directory.
- [ ] Logs and artifacts cover the key failure cases: discovery failure, selection cancellation, conflict escalation, validation failure, and publish failure.

## 4. Exit Criteria

- [ ] `Definition of Done` in [`index.md`](./index.md) is fully checked.
- [ ] Planned slices in [`proposed-prs.md`](./proposed-prs.md) are complete or explicitly deferred with rationale.
- [ ] Remaining risks in [`risks.md`](./risks.md) are accepted with owner and mitigation.
- [ ] The final validation path includes focused command proofs plus the repository gate `make all`.

# Implement the end-to-end `meta merge` command, dashboard, agent merge-run orchestration, and aggregate PR publication

Build the full repository-scoped `meta merge` workflow as one coherent feature. Add a new top-level `meta merge` command family in `src/cli.rs` and `src/lib.rs`, backed by a focused GitHub service that uses the `gh` CLI to resolve the current repository, list open pull requests targeting `main`, inspect PR metadata, and create or update an aggregate pull request. On top of that transport, implement a ratatui one-shot dashboard for multi-PR selection, with deterministic non-interactive render and JSON proof paths for tests. When a batch is launched, create a fresh aggregate branch from the latest `origin/main` in a safe workspace outside the source checkout, gather PR context, persist deterministic run artifacts under `.metastack/merge-runs/<RUN_ID>/`, and invoke the configured local agent to produce a merge strategy before execution. Then execute the plan by applying the selected PRs in an explicit order, invoking the agent to resolve merge conflicts and integration fixes as needed, rerunning repository validation, and publishing a single aggregate PR back to `main` that summarizes the included source PRs. Update repository docs and prompts as needed so the new workflow, `gh` prerequisite, local artifact layout, and validation path are explicit and maintainable.

## Acceptance Criteria

- `meta merge --help` documents the new command family and its one-shot dashboard behavior.
- The implementation discovers the current GitHub repository from the active checkout and fails with a clear error when `gh` auth or repo resolution is unavailable.
- A deterministic non-interactive proof path exists for tests, such as `meta merge --json` or equivalent, that emits the open PR metadata needed by the dashboard and planner.
- Integration tests stub `gh` and verify PR discovery, repo resolution, and error handling without calling the live GitHub API.
- Running `meta merge` in a TTY opens a dashboard that lists open PRs from the current repo and allows selecting multiple entries before launch.
- The dashboard includes a deterministic snapshot path such as `meta merge --render-once` with scripted events so layout and interaction states can be tested.
- The selected batch summary clearly shows which PRs will be handed to the merge agent and that the flow is one-shot rather than a persisted listener session.
- Tests cover empty-state, single-selection, multi-selection, and cancellation behavior.
- Launching a merge batch creates a fresh workspace rooted outside the source checkout and based on the latest `origin/main` for the current repository.
- Each run writes deterministic local artifacts under `.metastack/merge-runs/<RUN_ID>/` that include the selected PR set, relevant GitHub context, and the agent-produced merge plan.
- The planning prompt includes enough repo and PR context for the agent to choose an explicit merge order and call out likely conflict hotspots before execution.
- Tests verify the workspace safety rules, run artifact layout, and repeat-run behavior for multiple merge batches.
- The merge runner applies the selected PRs onto the aggregate branch in an explicit order and records per-PR progress and outcomes in the local run artifacts.
- When a merge conflict occurs, the command invokes the configured local agent in the aggregate workspace, captures the resolution steps, and either continues successfully or stops with a concrete blocker report.
- After the batch is merged locally, the command reruns the configured validation commands for the repo and refuses to publish the aggregate PR when validation fails.
- A successful run pushes the aggregate branch and opens or updates a GitHub PR back into `main` whose title and body identify the batched PRs included in the merge.
- Integration tests cover both a clean multi-PR batch and a conflict-requiring batch using stubbed `gh`, temp git repos, and deterministic validation commands.
- `README.md` documents how to run `meta merge`, what the dashboard does, the required `gh` setup, and what outputs are created locally and on GitHub.
- Any behavior or workflow-contract changes needed for agent-run merge orchestration are reflected in the relevant repo docs and prompts in this repository.
- The test suite includes an end-to-end proof that exercises command entry, dashboard launch or snapshot behavior, merge-run artifact creation, and aggregate PR publication through stubs.
- The feature’s validation path is explicit and reproducible with repository-local commands suitable for CI and manual verification.
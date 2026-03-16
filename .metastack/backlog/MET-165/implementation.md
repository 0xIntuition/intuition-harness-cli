# Implementation Plan

Last updated: 2026-03-16

## Workstreams

1. Add the `meta merge` CLI surface and the `gh`-backed repository or PR discovery contract.
2. Implement the one-shot dashboard, deterministic JSON output, and scripted `--render-once` proof path.
3. Build the safe merge runner, merge-run artifact writer, and agent planning or conflict-resolution flow.
4. Publish the aggregate PR, document the workflow, and finish the end-to-end test harness.

## Touchpoints

- CLI entrypoints: `src/cli.rs`, `src/lib.rs`
- New merge implementation area: `src/merge.rs` or `src/merge/*`
- GitHub transport helpers: new merge module plus shared process helpers where warranted
- Workspace safety and git operations: `src/listen/workspace.rs` or extracted shared helpers
- Agent execution: `src/agents/execution.rs`
- Repo-local paths and artifacts: `src/fs.rs`, `.metastack/merge-runs/<RUN_ID>/`
- Existing dashboard patterns to mirror: `src/setup.rs`, `src/sync_dashboard.rs`, `src/linear/dashboard.rs`
- Tests: `tests/merge.rs`, `tests/commands.rs`, `tests/support/common.rs`
- Docs and prompts: `README.md`, `WORKFLOW.md`, `prompts/injected-agent-workflow-contract.md`

## Sequencing

1. Lock the command-line contract and `gh` JSON contract first so downstream tests have stable fixtures.
2. Build dashboard state and snapshot coverage next so selection semantics are fixed before runner work starts.
3. Add the merge-run artifact schema before wiring agent planning and merge execution.
4. Implement clean-batch application first, then add conflict escalation and validation gating.
5. Finish aggregate PR publication, docs, and full-repo validation last.

## Constraints

- Keep behavior additive; do not regress existing command families.
- Prefer extracting shared workspace helpers over duplicating listener safety logic.
- Keep tests hermetic: no live GitHub API, no mutation of the source checkout, no dependence on a real TTY.

# Context Note: Current CLI, dashboard, and workspace patterns for `meta merge`

Last updated: 2026-03-16

## Source

- Link: `README.md`
- Link: `src/cli.rs`
- Link: `src/lib.rs`
- Link: `src/fs.rs`
- Link: `src/listen/workspace.rs`
- Link: `src/agents/execution.rs`
- Link: `.metastack/codebase/ARCHITECTURE.md`
- Link: `.metastack/codebase/TESTING.md`
- Captured on: 2026-03-16
- Source type: local repository evidence

## Summary

`metastack-cli` already has stable patterns for domain-first clap command families, deterministic `--json` and `--render-once` proof paths, ratatui snapshot testing with `TestBackend`, filesystem-first `.metastack/` state, subprocess-based local agent execution, and safe workspace cloning for unattended listener work. The missing capability is a repository-scoped GitHub merge batching command that uses those same patterns instead of inventing a separate execution model.

## Key Findings

1. `src/cli.rs` and `src/lib.rs` are the canonical places to add new top-level command families and their dispatch paths.
2. Existing dashboard-oriented commands already rely on `--render-once`, scripted events, and `ratatui::backend::TestBackend` for deterministic CI coverage.
3. `src/listen/workspace.rs` already enforces the key safety constraint needed here: do not run mutable git operations inside the source checkout.
4. `src/agents/execution.rs` is subprocess-first and already knows how to inject repo-local prompts, models, reasoning, and working directories into configured agents.
5. `src/fs.rs` owns repo-local path conventions under `.metastack/` and is the right place to add deterministic merge-run paths.
6. Repository testing patterns strongly prefer stubbed external boundaries, temp repos, and exact filesystem side-effect assertions over live-network proofs.

## Implications for Technical: Implement the end-to-end `meta merge` command, dashboard, agent merge-run orchestration, and aggregate PR publication

- Reuse the existing deterministic dashboard and agent-execution patterns rather than introducing a separate UI or process model.
- Extract or share workspace-safety helpers before implementing aggregate merge execution.
- Make `.metastack/merge-runs/<RUN_ID>/` a first-class path set in `src/fs.rs` so artifact assertions stay consistent.
- Keep GitHub coverage hermetic by stubbing `gh` through the test harness instead of adding live GitHub dependencies.

# Add `meta backlog review` with shared multi-agent review orchestration, Linear-first sync, and configurable defaults

Implement a new `review` command under the backlog family and back it with a reusable review pipeline that generalizes the current refinement flow instead of duplicating it.

Scope:
- add `review` under the backlog command family in [`src/cli.rs`](/Users/metasudo/workspace/metastack/stack/metastack-cli/src/cli.rs)
- dispatch it from [`src/lib.rs`](/Users/metasudo/workspace/metastack/stack/metastack-cli/src/lib.rs)
- extract or generalize the current refinement flow from [`src/linear/refine.rs`](/Users/metasudo/workspace/metastack/stack/metastack-cli/src/linear/refine.rs) into a shared review module that both `meta backlog review` and the existing issue-refinement path can reuse while preserving current `meta linear issues refine` semantics
- make the backlog review lifecycle Linear-first by pulling the latest issue state into `.metastack/backlog/<ISSUE>/` via the existing sync model in [`src/sync_command.rs`](/Users/metasudo/workspace/metastack/stack/metastack-cli/src/sync_command.rs), performing cumulative local rewrites, and pushing the final description back to Linear on success
- support configurable sequential multi-agent review passes such as `codex,claude,codex`, with repeats allowed, later passes consuming the latest rewritten ticket plus prior review outputs, and fallback to another working agent when a configured pass is unavailable or fails
- extend install-scoped config in [`src/config.rs`](/Users/metasudo/workspace/metastack/stack/metastack-cli/src/config.rs) and [`src/config_command.rs`](/Users/metasudo/workspace/metastack/stack/metastack-cli/src/config_command.rs) to expose backlog-review defaults for ordered agent chain, loop count, fallback behavior, and default mutation mode, while allowing per-run CLI overrides
- record auditable review artifacts under `.metastack/backlog/<ISSUE>/artifacts/review/<RUN_ID>/`, including before/after snapshots, pass-by-pass outputs, requested agent, actual agent used, and fallback reason when substitution happens
- update [`README.md`](/Users/metasudo/workspace/metastack/stack/metastack-cli/README.md) with command usage, critique-only and default-apply behavior, config examples, artifact layout, and fallback semantics
- add integration coverage for critique-only runs, default apply runs, multi-agent sequencing, fallback handling, and config serialization

Sequence the work so the shared engine and CLI surface land first, then multi-agent orchestration and fallback, then Linear-first apply/push behavior, and finally config/docs/tests.

## Acceptance Criteria

- `meta backlog review --help` documents issue identifiers, review-mode flags, and agent-sequencing overrides, and the command is wired through the existing backlog family.
- A shared review implementation is used by `meta backlog review` and the existing issue-refinement path so review logic is not copy-pasted into a second flow.
- Running `meta backlog review MET-35 --critique-only` writes a review run under `.metastack/backlog/MET-35/artifacts/review/<RUN_ID>/` and leaves the local backlog packet and Linear description unchanged.
- Config can express an ordered review chain with repeats, for example `codex,claude,codex`, plus a loop/pass count that controls how many review steps run.
- For a multi-pass run, pass 2 and later receive the prior pass findings and rewritten issue content as input, and the artifacts show the cumulative progression of the ticket.
- If a configured agent is unavailable or its pass fails, the command falls back to another configured or supported agent, records that substitution in the run artifacts, and only fails when no working agent can complete the pass.
- `meta backlog review MET-35` refreshes `.metastack/backlog/MET-35/index.md` from the current Linear issue before the first review pass begins.
- By default, a successful review run updates the local backlog packet and pushes the final description back to Linear; `--critique-only` writes artifacts only and does not mutate either destination.
- If the final local apply succeeds but the Linear update fails, the command exits non-zero, preserves the local rewrite plus before/after snapshots in the review artifact directory, and clearly reports that remote sync failed.
- `meta runtime config --json` includes backlog-review defaults, and persisted TOML can store and reload the review chain, loop count, and default apply/critique mode without breaking existing agent config behavior.
- README examples cover `meta backlog review <ISSUE>`, a critique-only invocation, and a configured multi-agent sequence with repeats and fallback behavior.
- Integration tests prove the command-path behavior for critique-only and default-apply flows, including `.metastack/backlog/<ISSUE>/artifacts/review/<RUN_ID>/` outputs and at least one agent-fallback scenario.
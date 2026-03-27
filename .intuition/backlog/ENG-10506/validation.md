# Validation Plan

## Command Proofs

- [x] `cargo test -q agents_build_help_describes_workspace_loop_and_flags`
- [x] `cargo test -q --test build`
- [x] `cargo test -q build::tests:: --lib`
- [x] `cargo clippy --all-targets --all-features -- -D warnings`
- [x] Limited validation to local command/help/runtime proofs; no Linear issue description mutation path was exercised.

## Notes

- `intu listen` must not overwrite the primary Linear issue description.
- Focused proofs covered the `agents build` help surface, sibling-workspace resolution, explicit `--dir` execution, build-route config resolution, working-directory enforcement, and git-repo validation failures.
- A later `make quality` sweep reran the new `agents build` coverage and the targeted lint/unit gates cleanly, but the aggregate run did not finish during observation because the existing `listen_once_relaunches_agent_until_issue_leaves_active_states` integration test continued running for an extended period. The same listen case also remained active when rerun in isolation with `cargo test --test listen listen_once_relaunches_agent_until_issue_leaves_active_states -- --nocapture`.

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

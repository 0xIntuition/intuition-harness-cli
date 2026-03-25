# Validation Plan

## Command Proofs

- `cargo test backlog_spec`
- `cargo test plan`
- `cargo test technical`
- `cargo test backlog_improve`
- `cargo test workflows`
- `cargo test linear`
- `cargo test listen`
- `cargo test merge_dashboard`
- `cargo test workspace_dashboard`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `make quality`
- Verify the original Linear issue description for `ENG-10425` remains unchanged
- Update the existing `## Codex Workpad` comment with validation notes instead of running `meta sync push`

## Notes

- `meta listen` must not overwrite the primary Linear issue description.
- 2026-03-25: `cargo test backlog_spec` passed after adopting the shared copy/export contract in `src/backlog_spec.rs`.
- Remaining validation is blocked on completing the broader rollout in `src/plan.rs`, `src/technical.rs`, `src/backlog_improve.rs`, the shared field consumers, and the dashboard surfaces.

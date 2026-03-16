# Validation Plan

Last updated: 2026-03-16

## Command Proofs

- `cargo run -- merge --help`
- `cargo run -- merge --json --root /tmp/meta-merge-proof-repo`
- `cargo run -- merge --render-once --root /tmp/meta-merge-proof-repo --event down --event space --event enter`
- `cargo test --test merge`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `make all`

## Harness Expectations

- Stub `gh` through `PATH` so no test hits the live GitHub API.
- Use temp git repositories with `origin/main` plus synthetic PR branches to prove clean and conflict-driven batches.
- Use deterministic validation commands inside temp repos so pass and fail publication behavior can both be asserted.
- Assert exact `.metastack/merge-runs/<RUN_ID>/` side effects, including plan, status, validation, and aggregate PR summary files.

## Notes

- Record exact stdout or stderr, exit code, active branch, workspace path, and aggregate PR title or body output for every proof.
- For conflict tests, capture both the pre-resolution blocker state and the post-agent continuation state.
- Never mutate the source checkout during validation; all merge execution must happen in the isolated aggregate workspace.

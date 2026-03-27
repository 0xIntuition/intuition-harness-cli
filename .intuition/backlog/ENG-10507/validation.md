# Validation

## Command Proofs

- [x] `cargo test build:: --lib`
- [x] `cargo test agents:: --lib`
- [x] `cargo clippy --all-targets --all-features -- -D warnings`
- [x] `cargo run -- agents build --help`
- [x] `cargo run -- agents build --dir <tmpdir> "test" --no-interactive`

## Notes

- Integration coverage exists in `tests/build.rs` for ticket workspace resolution, `--dir` mode, provider-route precedence, CLI override precedence, and the non-git workspace failure path.
- Additional regression coverage exists in `tests/commands.rs` for the `agents --help` surface and `agents build --help` flag contract.
- This session also reran `cargo test --test commands -- --nocapture` and `cargo test --test config -- --nocapture` after the CLI help update.
- Direct command proof: `cargo run -- agents build --help` printed `Usage: intu agents build [OPTIONS] [WORKSPACE] [PROMPT]` plus `--dir`, `--max-turns`, and `--no-interactive`.
- Direct command proof: `cargo run -- agents build --dir <tmp-workspace> "test" --root <tmp-repo> --no-interactive` completed successfully with a stub provider and printed the expected status line plus completion summary.
- This session could not update the Linear workpad comment because no Linear tool/auth surface is available in-session.

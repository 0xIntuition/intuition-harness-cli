# Validation Plan

## Command Proofs

- [x] baseline sizing: `wc -l src/linear/transport.rs src/linear/service.rs`
- [x] pull/sync evidence: `git fetch origin --prune`
- [x] transport unit tests: `cargo test linear::transport::tests --lib`
- [x] service unit tests: `cargo test linear::service::tests --lib`
- [x] Linear command integration tests: `cargo test --test linear`
- [x] Sync integration tests: `cargo test --test sync`

## Notes

- `git rev-list --left-right --count HEAD...origin/main` returned `0 0` before edits, so the branch started cleanly aligned with `origin/main`.
- The integration proofs exercise the refactored boundary through real command flows instead of only module-local tests.

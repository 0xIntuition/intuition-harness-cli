# Validation Plan

## Command Proofs

- [x] `cargo test --test commands -- --nocapture`
- [x] `cargo test --test build -- --nocapture`
- [x] `cargo test build:: --lib -- --nocapture`
- [x] `cargo clippy --all-targets --all-features -- -D warnings`
- [x] Limited validation to local command/help/runtime proofs; no Linear issue description mutation path was exercised.

## Notes

- `intu listen` must not overwrite the primary Linear issue description.
- Focused proofs covered the `agents build` help surface plus the local build-loop helpers added in `src/build.rs`.

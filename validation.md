# Validation — ENG-10518

## Commands

- `cargo test --test config -- --test-threads=1`
- `cargo test --test merge -- --test-threads=1`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `make quality`
- `cargo run -- merge --help`
- `cargo run -- runtime config --help`

## Results

- `cargo test --test config -- --test-threads=1`
  - passed
  - 27 tests green
  - proved `recent_main_limit` persists through config editing and rejects out-of-range values

- `cargo test --test merge -- --test-threads=1`
  - passed
  - 26 tests green
  - proved aggregate mode still writes one aggregate publication result
  - proved `--render-once --sequential --checkpoints` renders recent-main and active-step checkpoint context
  - proved non-interactive sequential runs write `state.json` plus `steps/<NN>-pr-<NUMBER>/` artifacts
  - proved `--resume-run` continues sequential runs from the first incomplete step without replaying completed publication work

- `cargo clippy --all-targets --all-features -- -D warnings`
  - passed

- `make quality`
  - passed
  - full repository gate green after updating the listen continuation assertion and GitHub stubs used by the broader listen suite

- `cargo run -- merge --help`
  - passed
  - confirmed `--sequential` and `--checkpoints`
  - confirmed merge help now describes aggregate and sequential publication

- `cargo run -- runtime config --help`
  - passed
  - confirmed `--merge-recent-main-limit`

## Notes

- Sequential validation now reuses the persisted validation command list from `context.json` during resume.
- Sequential publication preserves the first publish failure after validation so resume can retry that step without replaying earlier successful steps.
- Aggregate publication keeps the existing replay-friendly fallback behavior for transient `gh pr create` failures.

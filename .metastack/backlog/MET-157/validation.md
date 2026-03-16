# Validation Plan

## Command Proofs

- `cargo test --test cli -- --nocapture listen`
- `meta agents readiness --root . --team MET --project "MetaStack CLI" --limit 10`
- `meta agents readiness --root . --team MET --project "MetaStack CLI" --limit 10 --json`
- `meta agents listen --root . --team MET --project "MetaStack CLI" --once --max-pickups 3 --max-concurrency 2 --render-once`
- `meta listen sessions list`
- `meta listen sessions inspect --root .`
- `make all`

## Notes

- Record exact stdout or rendered summary, exit code, and filesystem side effects for each changed command path.
- Confirm the readiness command does not create workspaces, mutate backlog files, or change Linear state.
- Confirm bounded kickoff only provisions sibling workspaces under `<repo>-workspace/` and never executes inside the source repository.
- Capture one proof showing why a blocked or missing-context ticket was excluded.
- Capture one proof showing session visibility after a successful bounded kickoff.

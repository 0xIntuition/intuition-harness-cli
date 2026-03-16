# Validation Plan

## Mandatory Gates

- `cargo test`
- Focused command proofs for `meta providers list|test|doctor`
- Focused command proofs for `meta policy show|apply`
- Focused command proofs for `meta budgets status`
- Compatibility proof for an existing single-agent setup
- Filesystem proof for persisted usage, token, and cost events

## Command Proofs

- `meta providers list --root .`
- `meta providers list --root . --json`
- `meta providers doctor --root .`
- `meta providers doctor --root . --json`
- `meta providers test codex --root .`
- `meta policy show --root .`
- `meta policy show --root . --json`
- `meta policy apply --root . --approval-profile unattended --budget-profile default`
- `meta budgets status --root .`
- `meta budgets status --root . --json`

## Compatibility and Evidence Checks

- Run an existing single-agent config fixture through the new resolver and prove the same effective provider/model still launch.
- Verify the repo-local metadata written by `meta policy apply` stays in `.metastack/meta.json` and remains deterministic across repeated runs.
- Verify runtime launches append machine-readable usage/cost records under `.metastack/agents/sessions/`.
- Verify budget status reads the ledger rather than raw log text.

## Notes

- Record exact stdout, stderr, exit code, and filesystem side effects for each focused proof.
- Capture both the empty-ledger case and a seeded-ledger case for `meta budgets status`.
- Prefer checked-in fixtures and deterministic command output over screenshots.

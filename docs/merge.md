# `meta merge`

## Summary

`meta merge` supports two execution modes:

- Aggregate mode is still the default. It applies the selected PRs onto one aggregate branch, validates that combined result, and publishes one aggregate PR.
- Sequential mode is enabled with `--sequential`. It applies the selected PRs one step at a time, validates each stacked step branch, and publishes one step PR per successful step.

Both modes fetch a bounded recent-main summary from the remote default branch before planning and persist that data under `.metastack/merge-runs/<RUN_ID>/recent-main.json`.

## CLI Surface

Common examples:

```bash
meta merge
meta merge --json
meta merge --no-interactive --pull-request 101 --pull-request 102 --validate "make quality"
meta merge --no-interactive --sequential --pull-request 101 --pull-request 102 --validate "test -f one.txt" --validate "test -f two.txt"
meta merge --render-once --sequential --checkpoints --events enter
meta merge --resume-run 20260327T164533Z
```

Sequential-only flags:

- `--sequential`: enable per-step application and stacked publication.
- `--checkpoints`: require operator review before each sequential step.

Validation rules:

- `--checkpoints` requires `--sequential`.
- `--checkpoints` conflicts with `--json` and `--no-interactive`.
- `--resume-run` reuses the persisted mode from `context.json`; it does not need `--sequential`.

## Recent-Main Planning

Before planning, `meta merge` resolves the repository default branch, fetches the most recent merged PRs for that branch through `gh`, and stores the result in `recent-main.json`.

Install-scoped config:

```toml
[merge]
recent_main_limit = 10
```

- Default: `10`
- Allowed range: `1..=50`

Planner prompts, stored `plan.json`, and conflict-resolution prompts all reference this persisted recent-main context.

## Aggregate Mode

Aggregate mode keeps the existing contract:

- one isolated sibling workspace
- one aggregate branch
- one aggregate validation artifact
- one aggregate PR body
- one aggregate `publication.json`

Aggregate publication still retries transient remote failures according to `[merge].publication_retry_attempts`.

## Sequential Mode

Sequential mode extends the run contract:

- `state.json` records the next incomplete step and whether remaining checkpoints were skipped.
- `plan.json` stores one step per selected PR, including planner rationale and risk notes.
- `publication.json` becomes a run-level summary of the completed step publications.
- `steps/<NN>-pr-<NUMBER>/` stores step-local context, progress, validation, publication, and PR body artifacts.

Stacked publication contract:

1. Step 1 publishes into the repository default branch.
2. Step 2 publishes into step 1's branch.
3. Later steps publish into the previous successful step branch recorded in `state.json`.

Sequential validation runs against the cumulative stacked branch state for the current step, using the persisted validation command list recorded in `context.json`.

Sequential publication intentionally stops on the first publish failure after validation. That preserves a resumable step boundary and avoids replaying already completed steps.

## Checkpoints

Checkpoint mode is only available in sequential runs. The checkpoint surface shows:

- the active PR
- the next planned step
- recent-main context
- planner rationale and risk notes

Available actions:

- approve the next step
- stop the run and keep resume state
- continue the remaining steps without further checkpoints

`--render-once --sequential --checkpoints --events ...` provides deterministic text proofs for this surface in tests.

## Resume

`meta merge --resume-run <RUN_ID>` reads the stored mode and artifacts:

- aggregate resume revalidates the preserved aggregate workspace, repushes the aggregate branch, and updates the aggregate PR
- sequential resume continues from the first incomplete step and reuses the original validation command list stored in `context.json`

Completed sequential steps are not replayed during resume.

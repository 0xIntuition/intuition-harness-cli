# Workstreams: Technical: Add issue readiness scoring and safe multi-ticket kickoff for small teams

Last updated: 2026-03-16

Break implementation into focused workstreams that map directly to the current CLI and listen runtime.

## Current Workstreams

- Readiness evaluation and bounded kickoff runtime: [`./workstream-template.md`](./workstream-template.md)

## Workstream Naming

- `runtime-readiness-evaluator.md`
- `runtime-batch-kickoff.md`
- `docs-readiness-rollout.md`

## Workstream Quality Bar

1. Each workstream must map changes to concrete files under `src/`, `tests/`, or repo docs.
2. Each workstream must identify the exact command proofs needed before merge.
3. Each workstream must call out any workspace-safety or state-compatibility risk explicitly.
4. If the work expands beyond one PR, the workstream should point back to [`../proposed-prs.md`](../proposed-prs.md).

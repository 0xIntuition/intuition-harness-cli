# Artifact Index: Technical: Add issue readiness scoring and safe multi-ticket kickoff for small teams

Last updated: 2026-03-16

Store execution artifacts here for readiness heuristics, state-shape examples, command proofs, and dashboard snapshots.

## Current Artifacts

- Readiness scoring rubric and classification matrix: [`./artifact-template.md`](./artifact-template.md)

## Artifact Naming

- `core-readiness-rubric.md`
- `snapshot-listen-readiness-2026-03-16.md`
- `analysis-session-concurrency.md`

## Artifact Rules

1. Each artifact must name the repo surface it informs, such as `src/listen/mod.rs` or `src/listen/store.rs`.
2. If an artifact changes the delivery approach, link the relevant decision ID from [`../decisions.md`](../decisions.md).
3. If an artifact becomes the source of truth for rollout or validation, link it from [`../index.md`](../index.md) and [`../validation.md`](../validation.md).
4. Keep artifacts focused on this repository; do not capture cross-repo implementation plans here.

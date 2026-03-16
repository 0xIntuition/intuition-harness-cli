# Artifact Index: Technical: Technical: Add multi-provider registry, approval profiles, and budget governance for fleet operations

Last updated: 2026-03-16

Store execution artifacts here for the provider registry and runtime governance rollout. Artifacts should help reviewers verify the design and implementation of the new operator surfaces, compatibility path, and budget evidence model.

## Planned Artifact Types

- Config contract notes for the additive registry and profile schema.
- Command output snapshots for `meta providers`, `meta policy`, and `meta budgets` in both human-readable and `--json` modes.
- Compatibility proofs showing an existing single-agent repo still resolves the expected provider and model without migration.
- Filesystem snapshots proving where usage, token, and cost events are written under `.metastack/agents/sessions/`.

## Current Artifacts

- No artifacts are checked in yet for this backlog item.
- Start from the issue-specific template: [`./artifact-template.md`](./artifact-template.md)

## Suggested Artifact Names

- `analysis-config-registry-shape.md`
- `snapshot-providers-list-json.md`
- `snapshot-policy-show-json.md`
- `snapshot-budgets-status-json.md`
- `proof-single-agent-compatibility.md`
- `snapshot-usage-event-ledger.md`

## Artifact Rules

1. Each artifact must state the exact command, fixture, or source file used to produce it.
2. If an artifact influenced a design choice, link the relevant decision ID from [`../decisions.md`](../decisions.md).
3. If an artifact demonstrates acceptance criteria, link it from [`../index.md`](../index.md) and [`../validation.md`](../validation.md).
4. Prefer deterministic local outputs over screenshots when the command already has a text or JSON mode.

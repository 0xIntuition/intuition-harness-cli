# Artifact Index: Fix `meta agents listen` attachment/artifact context bootstrap noise for Todo issue pickup

Last updated: 2026-03-16

Store execution artifacts here: design sketches, schema drafts, trace logs, snapshots, and reference outputs.

## Current Artifacts

- Add artifact files and list them here.
- Start from template: [`./artifact-template.md`](./artifact-template.md)

## Artifact Naming

- `core-<topic>.md`
- `snapshot-<system>-<date>.md`
- `analysis-<question>.md`

## Artifact Rules

1. Each artifact should state purpose and owner.
2. If an artifact drives a decision, link the decision ID from `../decisions.md`.
3. If an artifact becomes authoritative, link it from `../index.md`.


# Example of Artifact

```
# Artifact: <name>

Last updated: 2026-03-16

## Purpose

What this artifact proves or documents.

## Inputs

- Input 1
- Input 2

## Output

Concise result summary.

## Linked Decisions

- Decision ID(s) in `../decisions.md`:

## Follow-ups

- [ ] Follow-up action 1
- [ ] Follow-up action 2
```
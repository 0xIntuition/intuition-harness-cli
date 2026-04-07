# Validation Evidence Guide

This file is intentionally stable. Do not record ticket-specific pull-request evidence here.

## Evidence Paths

| Path | Purpose | Mutability |
| --- | --- | --- |
| `.metastack/backlog/<ISSUE>/validation.md` | packet-local proof during agent execution | per-issue |
| `.metastack/backlog/<ISSUE>/artifacts/README.md` | packet-local artifact index | per-issue |
| `artifacts/validation/<TICKET>.md` | repo-level reviewer evidence for a ticket or PR | per-ticket |
| `artifacts/README.md` | stable repository artifact directory guide | stable |

## Rules

- Keep packet-local execution notes in `.metastack/backlog/<ISSUE>/validation.md`.
- Keep packet-local artifact indexes in `.metastack/backlog/<ISSUE>/artifacts/README.md`.
- Record repo-level reviewer-facing evidence in `artifacts/validation/<TICKET>.md`.
- Do not write ticket-specific evidence into repo-root `validation.md`.
- Do not update repo-root `artifacts/README.md` for routine per-PR evidence.

## Migration

The previous repo-root validation payload was migrated to:

- [`artifacts/validation/ENG-10510.md`](artifacts/validation/ENG-10510.md)

For the naming and structure convention, see [`artifacts/validation/README.md`](artifacts/validation/README.md).

# Repository Artifact Guide

This directory is intentionally stable. Do not update this file for routine per-PR validation evidence.

## Current Layout

- [`./validation/README.md`](./validation/README.md) — naming and structure convention for repo-level per-ticket validation evidence
- [`./validation/`](./validation/) — reviewer-facing validation evidence, one file per ticket
- root-level snapshot files — legacy historical artifacts retained for reference when they predate the per-ticket convention

## Rules

- Record new repo-level reviewer evidence in `validation/<TICKET>.md`.
- Do not write ticket-specific evidence into repo-root `validation.md`.
- Do not treat this file as a mutable per-PR artifact index.

## Legacy Artifacts

- [`./snapshot-listen-validation-gates-2026-04-06.md`](./snapshot-listen-validation-gates-2026-04-06.md) — historical ENG-10505 listen validation-gates snapshot retained at its original path

# Per-Ticket Validation Evidence

Store repo-level reviewer-facing validation evidence here, using one Markdown file per ticket.

## Naming

- Use `<TICKET>.md`.
- Examples: `ENG-10510.md`, `ENG-10742.md`.

## Scope

- Use this directory for repo-level PR evidence, command proofs, acceptance mapping, and reviewer notes.
- Keep packet-local execution notes in `.metastack/backlog/<ISSUE>/validation.md`.
- Keep packet-local artifact indexes in `.metastack/backlog/<ISSUE>/artifacts/README.md`.

## Recommended Structure

- Summary
- Command Proofs
- Results
- Notes
- Acceptance Criteria Mapping

## Rules

- Do not write ticket-specific evidence to repo-root `validation.md`.
- Do not update repo-root `artifacts/README.md` for routine per-PR evidence.

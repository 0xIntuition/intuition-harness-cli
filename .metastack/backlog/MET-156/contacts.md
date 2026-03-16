# Contacts: Technical: Technical: Add multi-provider registry, approval profiles, and budget governance for fleet operations

Last updated: 2026-03-16

## Owners

- Driver: MetaStack CLI runtime/config maintainer responsible for `src/config.rs`, `src/cli.rs`, and command dispatch.
- Reviewer: MetaStack CLI reviewer covering agent execution, listen behavior, and repo-local filesystem semantics.
- Stakeholder: Fleet operations owner who needs inspectable provider choice, approval posture, and budget status.

## Escalation

- Product: backlog owner for `MET-106` and the parent feature `MET-94`.
- Engineering: crate owners for command surface, config compatibility, and unattended runtime behavior.
- Operations: operator responsible for local agent availability, provider credentials, and budget policy defaults.

## Communication Notes

- Preferred review channel: reviewer comments on PR slices linked from the backlog item plus inline notes on command semantics and config shape.
- Release coordination notes: ship docs and compatibility proofs with the first implementation PR so operators can evaluate the migration path before runtime routing is enabled by default.

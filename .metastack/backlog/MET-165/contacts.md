# Contacts: Technical: Implement the end-to-end `meta merge` command, dashboard, agent merge-run orchestration, and aggregate PR publication

Last updated: 2026-03-16

## Owners

- Driver: the engineer assigned to the child technical issue created from parent `MET-161`.
- Reviewer: a `metastack-cli` maintainer who owns Rust CLI command-surface and TUI review.
- Stakeholder: the maintainer responsible for agent-backed repository workflows and GitHub publication behavior.

## Escalation

- Product: owner of parent issue `MET-161` and any follow-up scope decisions for `meta merge`.
- Engineering: maintainer for `src/cli.rs`, `src/lib.rs`, and shared workspace-safety utilities.
- Operations: maintainer responsible for local `gh` prerequisites, temp-repo test harnesses, and release-facing documentation.

## Communication Notes

- Preferred review channel: GitHub PR review with linked backlog docs from this directory.
- Release coordination notes: if `meta merge` changes install or prerequisite expectations, update `README.md` before merge and keep the `gh` dependency explicit.

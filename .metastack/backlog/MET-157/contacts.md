# Contacts: Technical: Add issue readiness scoring and safe multi-ticket kickoff for small teams

Last updated: 2026-03-16

## Owners

- Driver: ticket assignee for the implementation PR; expected to be the engineer touching `src/listen/*`, `src/config.rs`, and CLI wiring
- Reviewer: maintainer responsible for unattended agent orchestration and workspace safety
- Stakeholder: owner of parent issue `MET-145` and the small-team pilot workflow for `meta agents listen`

## Escalation

- Product: parent-issue owner if the readiness classes or downgrade language no longer match team triage expectations
- Engineering: listen/runtime maintainer if concurrency limits or session persistence semantics need a policy decision
- Operations: CLI maintainer if rollout requires changes to setup defaults, repo config, or local runtime docs

## Communication Notes

- Preferred review channel: PR review with links back to this backlog item and the parent Linear issue
- Release coordination notes: ship with README updates and explicit notes about new config keys, added command surfaces, and any behavior-preserving defaults

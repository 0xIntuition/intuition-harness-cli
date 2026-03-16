# Context Note: current listen readiness baseline

Last updated: 2026-03-16

## Source

- Link: `../../README.md`
- Captured on: 2026-03-16
- Source type: repository documentation
- Link: `../../src/listen/mod.rs`
- Captured on: 2026-03-16
- Source type: runtime orchestration implementation
- Link: `../../src/listen/state.rs`
- Captured on: 2026-03-16
- Source type: persisted session schema
- Link: `../../src/listen/store.rs`
- Captured on: 2026-03-16
- Source type: install-scoped listen project store
- Link: `../../src/listen/workspace.rs`
- Captured on: 2026-03-16
- Source type: workspace safety implementation

## Summary

The current unattended flow already claims Todo issues, prepares isolated sibling workspaces, persists session state, and exposes queue and session summaries through existing listen dashboards. Eligibility is still narrow and mostly boolean: required label, assignment scope, and whether an active session already blocks pickup. The repo has no first-class readiness scoring surface, no explicit explanation for missing context beyond skip notes, and no active-session concurrency ceiling separate from `max_pickups` per poll.

## Key Findings

1. `meta agents listen` already performs limited skip reasoning in `skip_reason`, but it only covers label and assignee filters and does not classify missing context or human-needed tickets.
2. The persisted listen state already tracks active and completed sessions in one install-scoped `session.json`, which is a natural place to reflect readiness-driven kickoff decisions without inventing a second state store.
3. Workspace provisioning is already safety-sensitive and anchored to a sibling `<repo>-workspace/<TICKET>/` root, so any batch kickoff must continue to route through the current workspace guardrails.
4. The public docs already teach `meta agents listen`, `meta listen sessions list`, and `meta listen sessions inspect --root .`, which makes those surfaces the correct visibility targets for readiness and bounded kickoff status.
5. Current Linear issue summaries include state, labels, assignee, comments, attachments, parent, and children, but dependency-state coverage may need a small transport expansion to satisfy the parent issue's dependency-filtering bar.

## Implications for Technical: Add issue readiness scoring and safe multi-ticket kickoff for small teams

- The backlog item should add a shared readiness evaluator and keep `meta agents listen` as the execution engine instead of creating a parallel orchestration stack.
- The ticket should add one read-only readiness command for explanation and one explicit concurrency control for pickup safety.
- Session visibility should build on `session.json`, existing dashboard summaries, and current inspect/list flows.
- Any transport expansion for dependencies should stay minimal and directly justified by the readiness contract.

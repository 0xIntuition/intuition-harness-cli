# Checklist: Technical: Add issue readiness scoring and safe multi-ticket kickoff for small teams

Last updated: 2026-03-16

## 1. Baseline and Decisions

- [ ] Confirm the ticket covers the full repository root and does not depend on sibling repositories.
- [ ] Confirm the readiness classes, numeric scoring intent, and non-goals in [`./index.md`](./index.md).
- [ ] Confirm the command and config contract in [`./specification.md`](./specification.md).
- [ ] Confirm ownership, review routing, and escalation expectations in [`./contacts.md`](./contacts.md).
- [ ] Confirm decisions `D-001` through `D-003` in [`./decisions.md`](./decisions.md) before implementation starts.

## 2. Implementation Tasks by Area

### Area: CLI and Config Surface

- [ ] Add a read-only readiness command surface for candidate Todo tickets.
- [ ] Add a bounded concurrency setting for `meta agents listen` with repo-config support in `.metastack/meta.json`.
- [ ] Preserve legacy `meta listen` compatibility where new listen flags apply.
- [ ] Document exact stdout or JSON output expectations for readiness results.

### Area: Readiness Evaluation and Linear Inputs

- [ ] Introduce a shared readiness evaluator under the existing listen flow instead of duplicating filter logic.
- [ ] Expand issue inputs to cover dependency and missing-context signals needed by the acceptance criteria.
- [ ] Distinguish hard blockers from downgrade-only reasons so the output can explain exclusions cleanly.
- [ ] Add deterministic tests for ready, blocked, missing-context, and human-needed classifications.

### Area: Bounded Batch Kickoff and Workspace Safety

- [ ] Enforce a max-concurrency limit against persisted active sessions before claiming additional issues.
- [ ] Preserve the current sibling workspace-root safety guarantees from `src/listen/workspace.rs`.
- [ ] Ensure bounded kickoff still records sessions, brief paths, backlog paths, workspace paths, and logs.
- [ ] Prevent partial kickoff state when classification or workspace validation fails.

### Area: Session Visibility, Dashboard, and Docs

- [ ] Expose readiness and exclusion reasons through existing listen/session summaries.
- [ ] Update the browser or render-once dashboard if queued-ticket visibility changes.
- [ ] Update user-facing docs for readiness scoring, concurrency config, and recommended command flows.
- [ ] Ensure backlog, README, and validation docs all describe the same command path.

## 3. Cross-Cutting Quality Gates

- [ ] Deterministic readiness scores and classes are verified for identical mocked issue inputs.
- [ ] No unsafe workspace deletion or source-repo execution path is introduced.
- [ ] Logs and session summaries include enough context to understand why a ticket was skipped or picked up.
- [ ] The new readiness checks do not require a separate persistence root outside the existing listen project store.
- [ ] Full validation covers dependency filtering, missing context, concurrency limits, and session visibility.

## 4. Exit Criteria

- [ ] Every item in `Definition of Done` in [`./index.md`](./index.md) is checked.
- [ ] PR slices in [`./proposed-prs.md`](./proposed-prs.md) are complete or explicitly deferred with rationale.
- [ ] Remaining risks in [`./risks.md`](./risks.md) have owners and mitigations.
- [ ] Final validation in [`./validation.md`](./validation.md) has recorded command proofs and the repo quality gate.

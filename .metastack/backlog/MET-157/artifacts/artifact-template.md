# Artifact: readiness-scoring-rubric

Last updated: 2026-03-16

## Purpose

Document the proposed readiness classes, scoring inputs, and downgrade reasons used by the new readiness evaluator that feeds `meta agents readiness` and bounded `meta agents listen` pickup.

## Inputs

- Current `meta agents listen` eligibility rules in `src/listen/mod.rs` (`required_label`, assignee scope, `max_pickups`)
- Session persistence model in `src/listen/state.rs` and `src/listen/store.rs`
- Workspace safety invariants in `src/listen/workspace.rs`
- Repo-local setup and context expectations in `.metastack/meta.json` and `.metastack/codebase/*.md`
- Parent issue `MET-145` acceptance criteria

## Output

Proposed first-pass rubric:

| Class | Typical score band | Hard gate | Example reasons |
|---|---:|---|---|
| `ready` | 80-100 | no blocking dependency, no missing required context, below concurrency cap | description present, acceptance criteria present, repo setup present |
| `blocked` | 0-40 | unresolved dependency or state prevents execution | dependency still active, already has active session, not in Todo |
| `missing_context` | 20-60 | issue can become ready after docs/context fixes | empty description, no validation guidance, missing repo codebase context |
| `human_needed` | 0-50 | agent execution would be unsafe or ambiguous | manual external dependency, unclear owner, conflicting instructions |

Required output fields for each candidate:

- `identifier`
- `title`
- `classification`
- `score`
- `reasons[]`
- `next_action`
- `would_pick_up`

## Linked Decisions

- `D-001`: shared readiness evaluator and read-only CLI surface
- `D-002`: concurrency cap enforced from persisted active sessions
- `D-003`: reuse existing listen/session persistence instead of new repo-local state files

## Follow-ups

- [ ] Confirm whether dependency signals come only from current issue relationships or require additional Linear transport fields.
- [ ] Capture one real `session.json` before/after example once implementation starts.
- [ ] Add a dashboard snapshot artifact if the queue or summary layout changes materially.

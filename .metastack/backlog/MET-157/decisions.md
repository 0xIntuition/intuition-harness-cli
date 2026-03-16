# Decisions: Technical: Add issue readiness scoring and safe multi-ticket kickoff for small teams

Last updated: 2026-03-16

Record meaningful scope, design, and rollout decisions here.

## Decision Log

### D-001: Add a shared readiness evaluator plus a read-only CLI surface

- Date: 2026-03-16
- Status: proposed
- Context: Current `meta agents listen` filtering is embedded inside the pickup loop and only reports limited skip notes.
- Decision: Introduce a shared readiness evaluation layer inside `src/listen/` and expose it through a new read-only command such as `meta agents readiness`, while reusing the same evaluator inside `meta agents listen`.
- Consequences: The scoring rules stay centralized, CLI users get an explicit explanation path before kickoff, and listen does not need a separate duplicate filter stack.

### D-002: Enforce bounded multi-ticket kickoff with an active-session concurrency cap

- Date: 2026-03-16
- Status: proposed
- Context: `max_pickups` limits how many new tickets can be claimed per poll, but it does not express the maximum number of active sessions allowed at once.
- Decision: Add a `max_concurrency` or equivalently named setting to the listen config and CLI surface, and enforce it against active non-completed sessions before new pickup occurs.
- Consequences: Small teams can safely launch several ready tickets without unbounded session growth, and current single-ticket behavior can remain the default when the new limit is `1`.

### D-003: Reuse the existing listen project store for readiness and kickoff visibility

- Date: 2026-03-16
- Status: proposed
- Context: The repo already persists listen state and project metadata in the install-scoped store resolved by `src/listen/store.rs`.
- Decision: Extend existing session or queue state with readiness-related metadata instead of creating a new repo-local state file.
- Consequences: `meta listen sessions inspect --root .`, dashboard rendering, and queued-ticket summaries can all surface the new information without extra storage plumbing.

### D-004: Keep the change narrow and avoid full listen modularization in this ticket

- Date: 2026-03-16
- Status: proposed
- Context: `src/listen/mod.rs` is already large, but there is a separate backlog item for lifecycle modularization.
- Decision: Permit only small extractions that are necessary for readiness evaluation or tests; defer broader structural work.
- Consequences: This ticket stays focused on behavior and safety, while larger architecture cleanup remains separately reviewable.

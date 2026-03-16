# Modularize `meta listen` Into Explicit Lifecycle Components

`src/listen/mod.rs` currently concentrates polling, issue claiming, workspace preparation, prompt rendering, attachment syncing, dashboard aggregation, session persistence, and worker supervision in one place. Split the listener into focused modules with explicit state transitions so future work on workspace safety, assignment rules, dashboards, and agent execution can land without colliding in the same file.

## Acceptance Criteria

- The `listen` entrypoint composes smaller modules for cycle orchestration, session/state management, workspace provisioning, backlog/context syncing, and dashboard assembly.
- Listener state transitions such as claimed, brief-ready, running, completed, and blocked are handled through clear typed helpers instead of broad cross-cutting free functions.
- Focused tests cover core lifecycle paths including claim-to-run progression, blocked/completed outcomes, and workspace safety protections.
# Extract a Shared Agent Planning Engine for `plan` and `technical`

`src/plan.rs` and `src/technical.rs` duplicate context loading, fenced JSON parsing, agent job orchestration, backlog file generation, and a large amount of TUI/session plumbing. Create a shared planning engine and reusable session primitives so new agent-backed planning flows can be added without copy-pasting another large command module.

## Acceptance Criteria

- A shared planning module owns context bundle loading, agent response parsing, backlog file rendering, and background job orchestration.
- `meta plan` and `meta technical` both use the shared engine while preserving current user-visible behavior and output contracts.
- Regression tests cover shared parsing and rendering utilities plus command-specific happy-path and failure-path behavior.
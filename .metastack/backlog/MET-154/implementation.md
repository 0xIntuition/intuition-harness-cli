# Implementation Plan

## Workstreams

1. Transport split
   - Replace the monolithic transport file with a stable facade plus resource modules.
   - Centralize HTTP GraphQL request handling and cursor pagination in shared helpers.
2. Service split
   - Keep the public `LinearService` constructor and methods stable.
   - Move catalog, issue workflow, resolution, workpad, and asset behavior into focused modules.
3. Test support
   - Replace the service-local fake with reusable builders and recording helpers.
   - Keep transport pagination coverage at the HTTP boundary and service mutation coverage at the fake boundary.

## Touchpoints

- CLI entrypoints:
  - `src/linear/command.rs`
  - `src/plan.rs`
  - `src/setup.rs`
  - `src/sync_command.rs`
  - `src/listen/mod.rs`
- Linear boundary:
  - `src/linear/transport.rs`
  - `src/linear/transport/*.rs`
  - `src/linear/service.rs`
  - `src/linear/service/*.rs`
- Tests:
  - `src/linear/transport/tests.rs`
  - `src/linear/service/tests.rs`
  - `tests/linear.rs`
  - `tests/sync.rs`

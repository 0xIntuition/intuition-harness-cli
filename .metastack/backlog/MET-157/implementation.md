# Implementation Plan

## Workstreams

1. Add a shared readiness evaluator, score model, and CLI/config contract for candidate Todo issues.
2. Integrate readiness results into bounded `meta agents listen` pickup without weakening current workspace safety or session persistence.
3. Surface readiness decisions through existing session and dashboard views, then validate the happy path and failure path end to end.

## Touchpoints

- CLI entrypoints:
  - `src/cli.rs`
  - `src/lib.rs`
- Listen runtime and state:
  - `src/listen/mod.rs`
  - `src/listen/state.rs`
  - `src/listen/store.rs`
  - `src/listen/dashboard.rs`
  - `src/listen/web.rs`
  - `src/listen/workspace.rs`
- Config and setup:
  - `src/config.rs`
  - `src/setup.rs`
- Linear issue inputs:
  - `src/linear/types.rs`
  - `src/linear/service.rs`
  - `src/linear/transport.rs`
- Docs and validation:
  - `README.md`
  - `docs/agent-daemon.md`
  - `tests/cli.rs`
  - module-local unit tests under `src/listen/*`

## Delivery Notes

- Favor a small shared helper or module for readiness logic rather than spreading more conditionals through the main listen loop.
- Preserve existing repo identity, workpad behavior, and workspace root guarantees.
- Treat command-proof coverage as part of implementation, not post-hoc documentation.

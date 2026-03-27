# Implementation Plan

## Workstreams

1. Finish the `agents build` runtime path in `src/build.rs`.
2. Tighten workspace validation, prompt-loop behavior, continuation handling, and completion summaries.
3. Update command help/docs and validate the changed command surface.

## Touchpoints

- CLI entrypoints: `src/cli.rs`, `src/lib.rs`, `src/build.rs`
- Agent/config routing: `src/config.rs`, `src/config_resolution.rs`
- Docs/tests: `README.md`, `build.rs`, `tests/commands.rs`
- `.intuition/backlog` files: `index.md`, `implementation.md`, `validation.md`

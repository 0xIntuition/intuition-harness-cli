# Specification: Add `agents build` command — interactive headless agent loop for workspace QA

Version: 0.1  
Last updated: 2026-03-26

Parent index: [`./index.md`](./index.md)

## 1. Executive Summary

Deliver `meta agents build` as a lightweight workspace QA loop for operators who already have a
workspace checkout and want to iterate with a headless agent without the full Linear/listen
ceremony.

## 2. Problem Statement

- Problem: there is no direct CLI path for iterative headless agent work inside an existing
  workspace checkout.
- Why now: workspace-based QA and fixup work currently requires heavier Linear-backed flows.
- Non-goals: persisting continuation handles outside the active `agents build` session.

## 3. Functional Requirements

1. Resolve the workspace from a sibling ticket workspace or explicit `--dir` path.
2. Run the selected provider in that workspace with interactive follow-up prompting.
3. Queue mid-run prompts and resume Codex natively when a continuation handle is available.

## 4. Non-Functional Requirements

- Performance:
- Reliability:
- Security:
- Observability:

## 5. Contracts and Interfaces

### 5.1 Inputs

- Input shape:
- Validation rules:

### 5.2 Outputs

- Output shape:
- Error shape:

### 5.3 Compatibility

- Backward-compat constraints:
- Migration plan:

## 6. Architecture and Data Flow

- High-level flow:
- Key components:
- Boundaries:

## 7. Acceptance Criteria

- [x] `meta agents build MET-45 "fix the auth bug"` resolves the sibling workspace and launches a
  headless agent there.
- [x] `meta agents build --dir /path/to/workspace "fix the auth bug"` accepts an explicit
  workspace directory.
- [x] Build runs stream stdout/stderr, print completion summaries, reuse provider config inside the
  loop, and keep continuation handles in-memory only for the active session.

## 8. Test Plan

- Unit tests: `cargo test -q build::tests:: --lib`
- Integration tests: `cargo test -q --test build`
- Contract tests: `cargo test -q agents_build_help_describes_workspace_loop_and_flags`
- Negative-path tests: explicit non-git workspace validation in `tests/build.rs`

## 9. Open Questions

1. Question
2. Question

## 10. Linked Workstreams

- Workstream A: [`./tasks/workstream-template.md`](./tasks/workstream-template.md)

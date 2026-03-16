# Checklist: Refactor Linear Integration Into Resource-Oriented APIs

Last updated: 2026-03-16

## 1. Baseline and Decisions

- [x] Confirm scope and non-goals in `index.md`.
- [x] Confirm contract boundaries in `specification.md`.
- [x] Capture a concrete baseline signal for the oversized transport/service files.

## 2. Implementation Tasks by Area

### Area: Linear Transport

- [x] Extract shared GraphQL request plumbing.
- [x] Extract shared cursor pagination plumbing.
- [x] Split resource-specific transport logic into smaller modules.

### Area: Linear Service

- [x] Keep `LinearService` as the stable facade for callers.
- [x] Move selection and resolution rules into focused internal helpers.
- [x] Keep workpad and attachment flows isolated from issue listing logic.

### Area: Tests

- [x] Add reusable service test support instead of a large in-file fake.
- [x] Cover pagination, project resolution, label creation, and issue mutation flows.
- [x] Re-run command-level integration tests that traverse the refactored boundary.

## 3. Cross-Cutting Quality Gates

- [x] Existing CLI-facing behavior preserved in targeted integration coverage.
- [x] No new config or auth paths introduced.
- [x] Pagination and mutation behavior verified after the module split.

## 4. Exit Criteria

- [x] Acceptance criteria in `index.md` are fully checked.
- [x] Validation commands in `validation.md` completed successfully.
- [x] Commit, push, and PR publication recorded.

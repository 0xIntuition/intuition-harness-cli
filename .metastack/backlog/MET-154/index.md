# Refactor Linear Integration Into Resource-Oriented APIs

## Summary

Split the Linear boundary into smaller resource-oriented transport modules and focused service helpers while preserving the existing caller-facing API used by CLI, sync, and listen flows.

## Scope

- Break `src/linear/transport.rs` into shared GraphQL and pagination plumbing plus resource modules for projects, issues, teams, viewer, labels, comments, uploads, and attachments.
- Break `src/linear/service.rs` into focused internal domains for catalog access, issue workflows, resolution rules, workpad updates, and asset flows.
- Replace the large in-file service fake with reusable test support that covers pagination, project resolution, label creation, workpad mutation, and issue mutation flows.

## Non-Goals

- No CLI flag or output contract changes.
- No changes to Linear config loading or auth behavior.
- No behavior changes outside the Linear transport/service boundary.

## Acceptance Criteria

- [x] The Linear transport layer is split into smaller modules for resources such as issues, projects, teams, labels, comments, and uploads, with shared request/pagination plumbing.
- [x] The service layer exposes focused operations and centralizes selection and resolution rules without leaking low-level transport concerns into callers.
- [x] Reusable test fixtures or fakes cover pagination, project resolution, label creation, and issue mutation flows.

## Evidence

- Reproduction signal before edits: `wc -l src/linear/transport.rs src/linear/service.rs` reported `1450` and `1003` lines respectively.
- Pull/sync result: branch `met-154-refactor-linear-integration-into-resource-oriented-apis` was already aligned with `origin/main` at `11487ab` after `git fetch origin --prune`.
- Publication: branch pushed to `origin/met-154-refactor-linear-integration-into-resource-oriented-apis` and PR `#71` opened with the `metastack` label.
- Validation highlights:
  - `cargo test linear::transport::tests --lib`
  - `cargo test linear::service::tests --lib`
  - `cargo test --test linear`
  - `cargo test --test sync`

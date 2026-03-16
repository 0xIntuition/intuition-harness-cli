# Specification: Refactor Linear Integration Into Resource-Oriented APIs

Version: 0.1  
Last updated: 2026-03-16

Parent index: [`./index.md`](./index.md)

## 1. Executive Summary

Refactor the Linear integration so transport concerns are organized by resource and service concerns are organized by domain rules, without changing the public APIs consumed elsewhere in the CLI.

## 2. Problem Statement

- Problem: `src/linear/transport.rs` and `src/linear/service.rs` had grown into large multi-responsibility files that mixed GraphQL documents, pagination, serde translation, selection rules, and workflow behavior.
- Why now: further Linear issue/project/label/workpad work would become harder to extend and test safely without smaller seams.
- Non-goals: changing CLI contracts, adding new Linear features, or altering configuration semantics.

## 3. Functional Requirements

1. Keep the existing `LinearClient`, `ReqwestLinearClient`, and `LinearService` caller-facing surface stable for existing consumers.
2. Move transport internals into smaller resource modules with shared request and pagination helpers.
3. Move service internals into focused helpers that own selection, resolution, workpad, and asset responsibilities.

## 4. Non-Functional Requirements

- Performance: pagination behavior must remain bounded by the existing page sizes and limit handling.
- Reliability: issue lookup, project resolution, label creation, uploads, and attachments must preserve current success/error behavior.
- Security: auth handling must remain unchanged and continue to use the configured Linear API key.
- Observability: no new logging behavior is required because this is an internal refactor with stable user-facing behavior.

## 5. Contracts and Interfaces

### 5.1 Inputs

- Input shape: existing `LinearClient` trait methods and `LinearService` public methods remain unchanged.
- Validation rules: retain existing team/project/state/label resolution behavior and mutation guardrails.

### 5.2 Outputs

- Output shape: existing `IssueSummary`, `ProjectSummary`, `TeamSummary`, `IssueComment`, and attachment flows remain unchanged.
- Error shape: preserve the current `anyhow`-based error messages for missing projects, missing states, failed GraphQL requests, and mutation failures.

### 5.3 Compatibility

- Backward-compat constraints: callers outside `src/linear` should not need code changes.
- Migration plan: internal-only refactor, so no migration steps are required.

## 6. Architecture and Data Flow

- High-level flow: caller -> `LinearService` facade -> focused service helper logic -> `LinearClient` trait -> `ReqwestLinearClient` resource module -> shared GraphQL/pagination helper -> Linear API.
- Key components:
  - shared transport plumbing: `graphql.rs`, `pagination.rs`, `model.rs`
  - transport resources: `projects.rs`, `issues.rs`, `teams.rs`, `viewer.rs`, `labels.rs`, `comments.rs`, `uploads.rs`, `attachments.rs`
  - service domains: `catalog.rs`, `issues.rs`, `resolution.rs`, `workpad.rs`, `assets.rs`
- Boundaries: callers see the same `LinearService` and `LinearClient` surface; only the internal module graph changes.

## 7. Acceptance Criteria

- [x] Transport logic is grouped into resource-oriented modules with shared request/pagination plumbing.
- [x] Service logic is grouped into focused helpers while preserving the public facade.
- [x] Reusable test support covers the refactored transport and service seams.

## 8. Test Plan

- Unit tests:
  - `cargo test linear::transport::tests --lib`
  - `cargo test linear::service::tests --lib`
- Integration tests:
  - `cargo test --test linear`
  - `cargo test --test sync`
- Contract tests: existing CLI integration coverage in `tests/linear.rs` exercises the unchanged public behavior.
- Negative-path tests: retained existing strict project resolution and command auth/config tests.

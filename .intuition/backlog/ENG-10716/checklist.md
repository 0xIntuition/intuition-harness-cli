# Checklist: Technical: Harden shared listen session flows against Linear failures and preserve resumability

Last updated: 2026-04-06

## 1. Baseline and Decisions

- [x] Confirm scope and non-goals in `index.md`.
- [x] Confirm contract boundaries in `specification.md`.
- [ ] Confirm owners/reviewers in `contacts.md`.

## 2. Implementation Tasks by Area

### Area: Core Package or Service

- [x] Implement core contract changes.
- [x] Add validation for inputs/config.
- [x] Add tests for happy-path and failure-path behavior.

### Area: Consumer Integrations

- [x] Update consuming apps/services.
- [x] Remove consumer-side ad hoc transforms.
- [x] Add integration compatibility tests.

### Area: Tooling and Docs

- [x] Update developer docs.
- [x] Add migration notes if contracts changed.
- [x] Ensure all links and references resolve.

## 3. Cross-Cutting Quality Gates

- [x] Deterministic behavior verified for identical inputs/config.
- [x] No forbidden dependencies or unsafe imports introduced.
- [x] Observability and logs cover key failure cases.
- [ ] Performance budget validated.

## 4. Exit Criteria

- [x] `Definition of Done` in `index.md` is fully checked.
- [ ] PR slices in `proposed-prs.md` are complete or explicitly deferred.
- [ ] Remaining risks in `risks.md` are accepted with owner + mitigation.

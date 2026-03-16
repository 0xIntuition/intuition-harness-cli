# Proposed PRs: Refactor Linear Integration Into Resource-Oriented APIs

Last updated: 2026-03-16

## PR Strategy

- Keep the refactor behavior-preserving and reviewable as a single internal-boundary change.
- Pair the module split with targeted proof coverage so reviewers can verify the unchanged contract quickly.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| MET-154-01 | Split Linear transport and service into focused internal modules while keeping public APIs stable | `src/linear/transport*`, `src/linear/service*`, `.metastack/backlog/MET-154/*` | None | Medium | `@kamescg` | in review (`#71`) |

## Merge Order

1. `MET-154-01`

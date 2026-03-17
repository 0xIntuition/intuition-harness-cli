# Proposed PRs: Unify built-in agent provider resolution, adapter architecture, and reasoning selection for Codex and Claude

Last updated: 2026-03-16

## PR Strategy

- Keep each PR independently reviewable.
- Land contract changes before consumer migration PRs.
- Avoid mixing behavior changes with broad refactors.

## Planned PRs

| PR ID | Goal | Files/Areas | Depends On | Risk | Owner | Status |
|---|---|---|---|---|---|---|
| unify-built-in-agent-provider-resolution-adapter-architecture-and-reasoning-selection-for-codex-and-claude-01 | Lock contract surface | `TBD` | None | Medium | `@tbd` | planned |
| unify-built-in-agent-provider-resolution-adapter-architecture-and-reasoning-selection-for-codex-and-claude-02 | Implement core behavior | `TBD` | unify-built-in-agent-provider-resolution-adapter-architecture-and-reasoning-selection-for-codex-and-claude-01 | Medium | `@tbd` | planned |
| unify-built-in-agent-provider-resolution-adapter-architecture-and-reasoning-selection-for-codex-and-claude-03 | Consumer alignment + tests | `TBD` | unify-built-in-agent-provider-resolution-adapter-architecture-and-reasoning-selection-for-codex-and-claude-02 | Low | `@tbd` | planned |

## Merge Order

1. `unify-built-in-agent-provider-resolution-adapter-architecture-and-reasoning-selection-for-codex-and-claude-01`
2. `unify-built-in-agent-provider-resolution-adapter-architecture-and-reasoning-selection-for-codex-and-claude-02`
3. `unify-built-in-agent-provider-resolution-adapter-architecture-and-reasoning-selection-for-codex-and-claude-03`

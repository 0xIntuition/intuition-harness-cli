# Context Index: Technical: Technical: Add multi-provider registry, approval profiles, and budget governance for fleet operations

Last updated: 2026-03-16

Use this folder for source-local research that clarifies how the current CLI resolves providers, launches agents, persists session state, and documents repo-scoped runtime behavior.

## Recommended Context Notes

- Current config contract in `src/config.rs`, especially `AgentSettings`, `PlanningAgentSettings`, and precedence rules.
- Agent launch behavior in `src/agents/execution.rs`, including built-in Codex sandbox/approval flags.
- Workflow and listen execution paths that currently resolve one provider/model pair per run.
- Existing session and telemetry persistence under `.metastack/agents/sessions/` and the listen state store.
- README and workflow docs that will need updates when operator governance surfaces ship.

## Current Notes

- No context notes have been added yet.
- Start from the issue-specific note template: [`./context-note-template.md`](./context-note-template.md)

## Authoring Rules

1. One note per topic, rooted in this repository's source or documentation.
2. Include exact file paths, command examples, and the date captured.
3. End each note with a short section explaining implications for this backlog item.
4. Keep references local unless an external provider API contract is required for a health check or pricing model.

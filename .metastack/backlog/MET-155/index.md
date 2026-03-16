# Establish Checked-In Codebase Context and Extension Guides for Agents

This repository currently lacks the generated planning context docs that the CLI itself expects agents to use, including `SCAN.md`, `ARCHITECTURE.md`, `CONVENTIONS.md`, `STACK.md`, `STRUCTURE.md`, and `TESTING.md`. Generate and curate the repo-local `.metastack/codebase` baseline, then document how contributors and agents should refresh and rely on those artifacts when extending the CLI.

## Acceptance Criteria

- The repository includes current `.metastack/codebase/*.md` context artifacts for this codebase, generated and then curated to be actually useful for future agent runs.
- Contributor-facing docs explain how to refresh, validate, and use the codebase context artifacts plus where to extend commands, listener flows, planning flows, and integrations.
- There is a lightweight validation path, command, or documented workflow that prevents the codebase context docs from silently drifting out of date.
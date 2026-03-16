# Workstreams: Add optional per-ticket agent selection for `meta agents listen` pickups

Last updated: 2026-03-16

Break implementation into focused workstream documents.

## Current Workstreams

- Add workstream files in this folder and list them here.
- Start from template: [`./workstream-template.md`](./workstream-template.md)

## Workstream Naming

- `runtime-<name>.md`
- `integration-<consumer>.md`
- `migration-<scope>.md`

## Workstream Quality Bar

1. Each workstream has clear in-scope and out-of-scope sections.
2. Each workstream maps tasks to concrete repo paths.
3. Each workstream states test expectations before implementation starts.


# Example of Task

```
# Workstream: <name>

Last updated: 2026-03-16

Parent index: [`../index.md`](../index.md)
Parent specification: [`../specification.md`](../specification.md)

## Objective

Describe the outcome for this workstream.

## Scope

In scope:
- Item 1
- Item 2

Out of scope:
- Item 1
- Item 2

## Files and Areas

- `path/to/file-or-folder`
- `path/to/file-or-folder`

## Implementation Tasks

- [ ] Task 1
- [ ] Task 2
- [ ] Task 3

## Test Plan

- Unit:
- Integration:
- Failure modes:

## Handoff Notes

- Current status:
- Next unblocker:
- Reviewer callouts:
```
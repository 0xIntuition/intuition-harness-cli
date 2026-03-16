# Workstreams: Technical: Technical: Add multi-provider registry, approval profiles, and budget governance for fleet operations

Last updated: 2026-03-16

Break implementation into focused workstream notes so config, operator surfaces, and runtime persistence can be reviewed independently.

## Suggested Workstreams

- Registry and compatibility normalization across install-scoped config and `.metastack/meta.json`.
- Provider diagnostics and capability inspection commands.
- Policy inspection/application and budget status surfaces.
- Runtime event persistence plus listen/workflow integration.

## Current Workstreams

- No workstream files have been added yet.
- Start from the issue-specific template: [`./workstream-template.md`](./workstream-template.md)

## Workstream Naming

- `runtime-provider-registry.md`
- `runtime-policy-surfaces.md`
- `runtime-budget-ledger.md`
- `integration-listen-and-workflows.md`

## Workstream Quality Bar

1. Each workstream must declare exact repo paths it intends to change.
2. Each workstream must name its acceptance and failure-path tests before implementation begins.
3. Each workstream must state compatibility expectations for existing single-agent repos when relevant.
4. Each workstream should stay reviewable enough to fit one planned PR slice from [`../proposed-prs.md`](../proposed-prs.md).

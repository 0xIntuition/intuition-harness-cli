# Add explicit automation permissions, dry-run tooling, and starter templates for powerful but safe repo-local jobs

## Problem
You want cron jobs to be powerful enough to call agents, CLIs, and external systems directly. The current implementation gives very little structure around what a job is allowed to do, and the operator UX is mostly limited to logs plus basic status output.

## Proposed change
Add a clear permission contract and better operator tooling around automation jobs. Jobs should be able to declare the tools or capabilities they expect, and engineers should have dry-run and template support so they can author advanced jobs without guessing how the runtime will behave.

## Scope
- Add declarative permission/capability fields to job definitions.
- Surface those permissions to agents and runtime execution in a consistent way.
- Add dry-run or simulation support that renders the effective trigger, steps, context, and permissions without mutating external systems.
- Ship starter templates for common high-value automations such as PR review and Linear backlog scoping.

## Acceptance Criteria

- Job definitions can declare explicit capabilities or permissions that are visible in the effective job contract and passed to runtime/agent execution in a documented way.
- A CLI dry-run path exists that lets an engineer inspect how a job would trigger and what steps would execute without performing write actions.
- The repo includes starter job examples or templates for GitHub PR review and Linear backlog scoping using the new automation model.
- Documentation explains the safety model, dry-run workflow, and how to author powerful jobs without falling back to opaque shell-only wrappers.
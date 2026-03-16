# Introduce a versioned automation job schema that extends `meta runtime cron` beyond schedule-only jobs

## Problem
Today `.metastack/cron/*.md` jobs support a single cron `schedule`, one optional shell `command`, and one optional `agent` prompt body. That is easy to author, but it does not leave room for richer trigger definitions, multi-step actions, tool permissions, or future hook-based entrypoints.

## Proposed change
Add a versioned automation job schema for repo-local Markdown job files that keeps the current authoring ergonomics while making room for richer runtime behavior. The loader should remain backward compatible with existing cron jobs, but support new top-level concepts such as `triggers`, `steps`, `permissions`, `retry`, and `concurrency`.

## Scope
- Define the new schema and normalization rules in Rust.
- Keep existing `.metastack/cron/*.md` files valid without migration.
- Add CLI inspection/validation so engineers can see the effective job contract before running it.
- Update README and cron docs to explain the old and new shapes clearly.

## Acceptance Criteria

- Existing `.metastack/cron/*.md` jobs that use only `schedule`, `command`, `agent`, and Markdown prompt body still load and run without behavioral regression.
- The cron loader can parse a richer versioned job definition with explicit trigger and step sections and normalize it into one internal runtime model.
- A repo-local CLI path exists to validate or explain a job definition and print the effective normalized contract without executing the job.
- README documentation for `meta runtime cron` is updated with the new schema, backward-compatibility rules, and at least one example of an advanced job definition.
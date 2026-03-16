# Backlog Item: Technical: Add issue readiness scoring and safe multi-ticket kickoff for small teams

This backlog item scopes the readiness-evaluation and bounded multi-ticket kickoff work for the `metastack-cli` repository.

## Required Files

- [`./index.md`](./index.md): summary, scope, proposed approach, and definition of done
- [`./specification.md`](./specification.md): technical contract for readiness classification, kickoff gating, and persistence
- [`./checklist.md`](./checklist.md): implementation checklist by area
- [`./contacts.md`](./contacts.md): ownership and escalation expectations
- [`./proposed-prs.md`](./proposed-prs.md): reviewable PR slices and sequencing
- [`./decisions.md`](./decisions.md): design and rollout decisions
- [`./risks.md`](./risks.md): active risks and open questions
- [`./implementation.md`](./implementation.md): concrete touchpoints across CLI, listen, Linear, session state, and docs
- [`./validation.md`](./validation.md): command proofs and final quality gate

## Supporting Folders

- [`./context/README.md`](./context/README.md): index of background notes for the current listen/session baseline
- [`./context/context-note-template.md`](./context/context-note-template.md): captured repo evidence for current readiness gaps
- [`./tasks/README.md`](./tasks/README.md): workstream index
- [`./tasks/workstream-template.md`](./tasks/workstream-template.md): execution plan for readiness evaluation and batch kickoff
- [`./artifacts/README.md`](./artifacts/README.md): artifact index
- [`./artifacts/artifact-template.md`](./artifacts/artifact-template.md): readiness rubric artifact

## Scope Notes

- Repository scope is the full `metastack-cli` root.
- The implementation should stay inside this repository and its repo-managed `.metastack/` workspace.
- Primary runtime touchpoints are expected in `src/cli.rs`, `src/lib.rs`, `src/config.rs`, `src/setup.rs`, `src/listen/*`, `src/linear/*`, `README.md`, and targeted tests under `tests/`.
- Workspace safety rules from `src/listen/workspace.rs` remain non-negotiable.

## Editing Notes

1. Keep the change narrowly scoped to readiness scoring, bounded kickoff, session visibility, and related docs/tests.
2. Preserve current default `meta agents listen` behavior unless an explicit readiness or concurrency setting changes it.
3. Reuse existing listen/session surfaces instead of inventing a separate persistence system.
4. Treat deeper `src/listen/mod.rs` modularization as related but out of scope for this ticket unless a small extraction is required for testability.

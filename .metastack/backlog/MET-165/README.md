# Backlog Item: Technical: Implement the end-to-end `meta merge` command, dashboard, agent merge-run orchestration, and aggregate PR publication

Last updated: 2026-03-16

This directory is the canonical local planning set for the technical child of parent issue `MET-161` inside the `metastack-cli` repository.

## Required Files

- `index.md`: scope, milestones, risks, validation bar, and definition of done for the `meta merge` slice.
- `specification.md`: contract for the command surface, `gh` transport, dashboard behavior, workspace safety, merge-run artifacts, and aggregate PR publication.
- `checklist.md`: execution checklist grouped by CLI surface, dashboard, runner/orchestration, and docs.
- `contacts.md`: role-based ownership and escalation notes for this repository-scoped change.
- `proposed-prs.md`: intended implementation slices and merge order.
- `decisions.md`: decision log for command shape, GitHub transport, artifact layout, and agent orchestration boundaries.
- `risks.md`: active delivery risks, mitigations, and open questions.
- `implementation.md`: sequencing and concrete repo touchpoints.
- `validation.md`: explicit command proofs and quality gates.

## Supporting Folders

- `context/`: repository evidence that informs the design.
- `tasks/`: workstream-level implementation plan.
- `artifacts/`: snapshots, run-layout drafts, and proof outputs created during execution.

## Editing Notes

1. Keep scope inside `/Users/metasudo/workspace/metastack/stack/metastack-cli` only.
2. Default to the full repository root unless a later child issue narrows the slice further.
3. Prefer concrete references to current repo modules such as `src/cli.rs`, `src/lib.rs`, `src/fs.rs`, `src/listen/workspace.rs`, and `src/agents/execution.rs`.
4. Preserve the repository's established deterministic test patterns: `--json`, `--render-once`, `ratatui::backend::TestBackend`, and stubbed external tools.
5. Document any required prompt or workflow-contract updates in this repository when merge orchestration behavior changes.

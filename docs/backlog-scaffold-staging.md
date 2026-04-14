# Backlog Scaffold Staging

This note records the deferred altitude-1 backlog organization path so current POC work can stay compatible with the later CLI surface without implementing it early.

## Status

`backlog ingest` and `backlog scaffold` are not active commands in this repository today. The active backlog command surface remains `backlog spec`, `backlog plan`, `backlog split`, `backlog improve`, `backlog tech`, and `backlog sync`. The hidden top-level `scaffold` command only creates repo-local `.metastack/` files and is unrelated to the future `backlog scaffold` product workflow.

## Altitude Boundaries

Altitude 0 is per-story shaping. It turns source material such as meetings, notes, or a single idea into draft product stories. This can stay agent-side so it can be invoked from any coding-agent session without requiring the harness binary.

Altitude 1 is backlog organization. It takes many shaped stories and organizes them into mutually exclusive, collectively exhaustive projects with milestones, likely leads, cross-cutting flags, and deduplication against existing Linear backlog. This is the long-term `backlog scaffold` scope.

Altitude 2 is engineering execution. It turns accepted product work into technical backlog, workspace execution, validation, pull requests, and review handoff through the existing `backlog tech` and `agents listen` paths.

## Staged Evolution

Stage 1, Cycle 88 throwaway ingest, is a one-off bridge for importing Greg's hand-shaped backlog. It has no reusable CLI contract and should not force the future command design.

Stage 2, post-POC `backlog ingest`, is the first reusable CLI step. It accepts a shaped markdown file from altitude-0, hand-authored altitude-1 output, or a future scaffold run, then writes Linear issues under the reviewed project, milestone, and lead assignments. This stage replaces throwaway scripts but does not generate the organization itself.

Stage 2 is gated by Linear milestone primitives in the transport layer. It also needs deterministic project and lead resolution, preservation of cross-cutting notes, umbrella-ticket linking, dry-run output, and idempotency or duplicate-detection rules before it can safely write to Linear.

Stage 3, `backlog scaffold`, is the altitude-1 organizer. It should accept many altitude-0 draft stories or a high-level concept, propose project and milestone groupings, flag cross-cutting work, identify likely leads, deduplicate against existing backlog, and require interactive review before any Linear writes. The reviewed output can flow through the Stage 2 ingestion path.

Stage 4 extends scaffold beyond the first organizer. Candidate scope includes cross-project milestone rollups, initiative awareness, goal-to-milestone mapping, generative concept-to-ticket mode, and umbrella-ticket rewrites when the input is an existing Linear issue.

## Compatibility Rules

Do not add a `backlog scaffold` subcommand until altitude-1 behavior has a reviewed input and output contract. Current POC work should preserve enough structure in shaped markdown to carry project, milestone, lead, cross-cutting, and source-story metadata forward.

Do not fold altitude-0 story shaping into the CLI by default. The current leaning is to keep `/meeting-to-stories` separate because it is agent-side shaping, while scaffold is CLI-side organization and Linear writing.

Do not make `backlog split` depend on the future scaffold design. `backlog split` decomposes one existing Linear issue; `backlog scaffold` organizes many draft product stories. Whether scaffold eventually reuses split internals should be revisited during Stage 3.

Do not conflate `backlog improve` with scaffold. `backlog improve` refines one existing ticket for hygiene; scaffold generates and organizes new backlog. They may share rubric artifacts later, but they have different mutation surfaces.

## Deferred Questions

- Does `backlog ingest` remain a separate command, or does it become a mode inside `backlog scaffold` after Stage 3 exists?
- What exact shaped-markdown schema is stable enough for altitude-0 output, hand-authored Cycle-style documents, and scaffold review output?
- Which Linear milestone operations are required first: lookup, create, update, issue assignment, ordering, or cross-project rollup?
- How should lead resolution work when names, aliases, and Linear users do not match exactly?
- What duplicate-detection threshold is safe enough to prevent repeated imports without hiding legitimate parallel work?
- When scaffold input is an existing umbrella issue, which parts of the parent description are rewritten and which metadata must be preserved?
- How should cross-functional flags be represented in Linear: labels, notes, child issue sections, milestones, or a separate reviewer artifact?


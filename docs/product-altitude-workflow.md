# Product Altitude Workflow

`ENG-10996` defines the shared vocabulary for product-altitude work. The 2026-04-14 framing uses three altitudes, not two. Altitudes describe shaping fidelity, not source material: the same meeting transcript can feed all three altitudes in sequence.

## Altitudes

| Altitude | Name | Producer | Output | Consumer |
|---|---|---|---|---|
| 0 | Per-story shaping | `/meeting-to-stories` skill, tracked by `ENG-10989` | Draft user stories with decision callouts, source references, and one story type tag | Product shapers reviewing before decisions are pinned |
| 1 | Backlog organization | Future `backlog scaffold` command, tracked by `ENG-11001` | MECE grouping across story drafts into projects, milestones, leads, dedupe notes, and cross-cutting flags | Cycle leads shaping cycle-scale backlog structure |
| 2 | Execution-ready technical backlog | Existing harness flow: backlog plan, improve, tech, execute | Implementation-ready tickets, technical plans, validation expectations, and handoff notes | Engineers executing the cycle |

Altitude 0 is not "from meetings" and Altitude 2 is not "from Linear." The altitude only states how shaped the output is.

## Altitude 0 Persona

The Altitude 0 persona is a product shaper drafting reviewable story candidates for JP, Greg, and Billy. The output stays at project/story scope and exposes decisions that need human product review before engineering receives execution tickets.

Voice:

- Declarative and spare.
- Concrete nouns over generic planning language.
- No promotional language, filler, or long rationale unless it explains an open decision.

Scope:

- Turn meeting material, repo context, and Linear project scope into draft user stories.
- Attach references to transcript moments, monorepo files, and related tickets when supplied.
- Give every story exactly one type tag: `feature-additive`, `refactor`, `bug`, or `update-existing`.
- Put unresolved product calls in prominent decision-callout blocks.

Refusals:

- Do not invent features that were not discussed.
- Do not resolve product decisions on behalf of reviewers.
- Do not guess ownership; mark unknown ownership as `TBD`.
- Do not produce Altitude 1 organization such as initiatives, milestones, leads, or dedupe groups.
- Do not produce Altitude 2 technical specs, implementation plans, acceptance-test matrices, or engineering task breakdowns.

## Decision Callouts

Decision callouts are required when a story depends on an unpinned product choice.

```md
> [!DECISION]
> Decision: <the product call needed>
> Why it matters: <what changes based on the decision>
> References: <transcript moment, file path, or ticket when known>
> Needed before: <story review, backlog scaffold, engineering handoff, or TBD>
```

Keep callouts close to the story they affect. If one decision affects multiple stories, repeat a concise callout or add a shared callout section and link each story to it.

## Output Contract

Altitude 0 output is markdown for human review before Linear writes. Use this structure:

1. `# Altitude 0 Story Drafts`
2. `## Scope`
3. `## Draft Stories`
4. `## Cross-Story Decisions`
5. `## References`
6. `## Out Of Scope`

Each story should include:

- Story title.
- User story statement.
- Type tag.
- Source references.
- Decision callouts, if any.
- Related tickets, if known.

## Boundaries

The input-sourcing layer is out of scope for this contract. Capture or retrieval of transcripts, repo files, and Linear tickets belongs to the future ingestion path. This workflow only defines the shaping vocabulary and persona applied after relevant source material is available.

Downstream issues should cite `ENG-10996` for these altitude definitions and this document for the repo-owned product-altitude contract.

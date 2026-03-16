# Technical: Add issue readiness scoring and safe multi-ticket kickoff for small teams

## Parent Issue

- Identifier: `MET-145`
- Title: Add issue readiness scoring and safe multi-ticket kickoff for small teams
- URL: https://linear.app/metastack-backlog/issue/MET-145/add-issue-readiness-scoring-and-safe-multi-ticket-kickoff-for-small

## Context

`metastack-cli` already has an unattended execution engine in `meta agents listen`, an install-scoped session store, a browser and render-once dashboard, and workspace provisioning that clones each ticket into a sibling `<repo>-workspace/<TICKET>/` checkout. What it does not yet have is a first-class way to tell a small team which Todo tickets are actually safe to launch, why other tickets were excluded, or how many tickets can run concurrently without overwhelming review bandwidth or risking unsafe workspace churn.

## Scope

In scope:

- readiness classification for Todo candidates using Linear metadata plus repo-local setup and context signals
- a read-only CLI surface that explains readiness score, class, and downgrade reasons
- bounded multi-ticket kickoff through the existing listen flow with an explicit concurrency limit
- persisted session visibility for readiness-driven kickoff results through current listen/session surfaces
- docs and tests covering dependency filtering, missing context, concurrency, and workspace safety

Out of scope:

- a full redesign of the listen dashboard or session store
- cross-repo orchestration or multi-host lease coordination
- broad `src/listen/mod.rs` modularization beyond what is necessary to land this feature safely

## Proposed Approach

1. Introduce a shared readiness evaluator under `src/listen/` that scores each Todo issue and classifies it as `ready`, `blocked`, `missing_context`, or `human_needed`.
2. Expose the evaluator through a read-only command such as `meta agents readiness --root . --team MET --project "MetaStack CLI" --limit 25` so operators can inspect the queue before launching work.
3. Extend `meta agents listen` to reuse the same evaluator and enforce `--max-concurrency <N>` against currently active sessions before claiming new issues.
4. Persist enough readiness and kickoff context in the existing listen project store so `meta listen sessions inspect --root .`, queued-ticket summaries, and dashboard views can explain why tickets were picked up, skipped, or deferred.
5. Keep workspace creation routed through `src/listen/workspace.rs` so new batch kickoff behavior cannot write in the source repository or remove directories outside the configured workspace root.

## Risks

- The current listen runtime is already large, so readiness work could increase coupling unless the evaluator is isolated behind typed helpers.
- Dependency-state signals may require a small Linear transport expansion.
- Operators may confuse `max_pickups` with the new concurrency limit unless the docs and output wording are explicit.

## Validation

- [ ] `cargo test --test cli -- --nocapture listen`
- [ ] `meta agents readiness --root . --team MET --project "MetaStack CLI" --limit 10`
- [ ] `meta agents readiness --root . --team MET --project "MetaStack CLI" --limit 10 --json`
- [ ] `meta agents listen --root . --team MET --project "MetaStack CLI" --once --max-pickups 3 --max-concurrency 2 --render-once`
- [ ] `meta listen sessions inspect --root .`
- [ ] `make all`

## Definition of Done

- [ ] Todo candidates can be classified as ready, blocked, missing context, or human needed with explicit reasons.
- [ ] A bounded multi-ticket kickoff path exists and respects a configurable active-session concurrency limit.
- [ ] Readiness output explains why tickets were downgraded or excluded and how to improve them.
- [ ] Session persistence and session-inspection surfaces reflect readiness-driven kickoff outcomes.
- [ ] Tests and command proofs cover dependency filtering, missing context, concurrency limits, and workspace safety.

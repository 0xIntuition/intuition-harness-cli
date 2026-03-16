# Risks: Technical: Add issue readiness scoring and safe multi-ticket kickoff for small teams

Last updated: 2026-03-16

## Active Risks

| Risk | Impact | Likelihood | Mitigation | Owner | Status |
|---|---|---|---|---|---|
| Readiness logic further bloats `src/listen/mod.rs` and makes lifecycle regressions harder to spot | High | Medium | Isolate scoring and explanation logic behind focused helpers or a small module; keep broad modularization out of scope | implementation driver | open |
| Current Linear issue payloads may not expose enough dependency-state detail for reliable filtering | High | Medium | Add only the minimal transport or service expansion needed for dependency gates and cover it with fixtures | implementation driver | open |
| Operators may misread `max_pickups` and `max_concurrency` as the same control | Medium | High | Use distinct names in CLI help, setup UI, README docs, and session output | reviewer + docs owner | open |
| Batch kickoff could accidentally erode workspace safety guarantees during reuse or recreation flows | High | Low | Keep all workspace provisioning routed through existing guards in `src/listen/workspace.rs` and add explicit regression tests | implementation driver | open |
| Session visibility could become noisy if readiness reasons overwhelm current dashboard summaries | Medium | Medium | Keep detailed reasons in inspect and JSON output, and use compact summaries for dashboard rows | reviewer | open |

## Open Questions

1. Should the readiness command support both table output and structured JSON in the first implementation, or is JSON acceptable behind a follow-up flag only?
2. Is parent/child issue state sufficient for the first dependency gate, or does the acceptance bar require blocked-by relations from Linear as well?
3. Should readiness degrade when `.metastack/codebase/SCAN.md` is stale, or only when the repo-local context is entirely missing?

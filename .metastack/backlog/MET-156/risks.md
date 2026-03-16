# Risks: Technical: Technical: Add multi-provider registry, approval profiles, and budget governance for fleet operations

Last updated: 2026-03-16

## Active Risks

| Risk | Impact | Likelihood | Mitigation | Owner | Status |
|---|---|---|---|---|---|
| Provider routing logic becomes duplicated across backlog, workflow, scan, and listen flows | High | Medium | Centralize resolution and fallback decisions in one shared runtime module and add regression tests at each command edge | `runtime-config` | open |
| Legacy single-agent config breaks silently when the registry schema is introduced | High | Medium | Keep the old fields as a normalized compatibility path and add explicit compatibility fixtures in `tests/config.rs` and command-level proofs | `runtime-config` | open |
| Budget status is misleading because usage events and rendered totals use different sources | High | Medium | Make the append-only event ledger the only source of truth for `meta budgets status` and dashboard rollups | `runtime-governance` | open |
| Active provider diagnostics are flaky in CI or offline environments | Medium | Medium | Separate static `doctor` checks from active `test` probes and gate network-dependent assertions carefully in tests | `cli-runtime` | open |
| Approval-profile semantics drift between Codex, Claude, and custom command providers | Medium | Medium | Define provider-independent policy intent first, then require adapters to declare supported mappings and produce actionable errors when unsupported | `runtime-governance` | open |

## Open Questions

1. Should the first version persist one global event ledger for the repo, one ledger per run, or both a ledger and per-run manifests?
2. Where should model pricing metadata live: alongside provider definitions, in a separate model catalog, or as built-in defaults with config overrides?
3. Should `meta policy apply` write only repo-local selections in `.metastack/meta.json`, or should it also support install-scoped profile changes through `meta runtime config` in a later slice?
4. Does the first implementation need dashboard integration beyond status commands, or is durable local JSON evidence sufficient for this milestone?

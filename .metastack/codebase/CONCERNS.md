# Codebase Concerns
**Analysis Date:** 2026-03-16

## Security Issues
- High: `.env` is committed and contains a `LINEAR_API_KEY` export.
  - Impact: anyone with repo access can use the token against Linear until it is revoked.
  - Recommendation: revoke/rotate the token, remove `.env` from git history, add `.env` to local-only setup docs, and enable secret scanning.
- Medium: agent and cron execution are intentionally shell/process based in `src/agents/execution.rs`, `src/cron/runtime.rs`, and `src/listen/worker.rs`.
  - Impact: a malicious repo config, cron job, or agent definition can run arbitrary local commands.
  - Recommendation: treat repo config as trusted input, warn loudly before executing repo-provided commands, and consider an allowlist or explicit safe mode for unattended runs.
- Low: the browser dashboard in `src/listen/web.rs` binds to `127.0.0.1` without an auth token.
  - Impact: acceptable on a local dev box, but forwarded/shared environments could expose issue titles, URLs, and session state.
  - Recommendation: keep loopback default, document the trust boundary, and add an optional random auth token if remote forwarding is expected.

## Performance Bottlenecks
- High: filtered issue listing can force a full Linear scan in `src/linear/service.rs` by calling `list_all_issues()` and filtering client-side.
  - Impact: larger Linear projects will pay unnecessary network and JSON decode cost for commands like `meta linear issues list`, `meta listen`, and sync dashboards.
  - Recommendation: push team/project/state filters into GraphQL queries where possible instead of fetching all issues first.
- Medium: sync pushes in `src/sync_command.rs` delete and recreate managed attachments on every change.
  - Impact: large backlog directories amplify upload time and churn attachment history.
  - Recommendation: diff by path/content hash and skip unchanged attachment uploads.
- Medium: listener/workspace refresh does repeated `git fetch` and hard reset work in `src/listen/workspace.rs`.
  - Impact: frequent polling across many tickets can spend noticeable time in git even when no work changes.
  - Recommendation: batch fetches per cycle or short-circuit workspace refresh when the branch already matches `origin/main`.

## Test Coverage Gaps
- Medium: the suite is strong on CLI integration, but there is no real end-to-end coverage for GitHub PR publishing, `gh` interactions, or release publication.
  - Impact: the highest-risk automation path in unattended ticket execution still depends on manual confidence.
  - Recommendation: add focused publish-path tests around PR creation/labeling/attachment behavior with hermetic stubs.
- Medium: no tests assert timeout/retry behavior for slow or flaky Linear requests.
  - Impact: network stalls may degrade the listener or interactive flows without a clear regression signal.
  - Recommendation: add transport-level tests once request timeouts/backoff exist.
- Low: no dedicated security regression tests cover secret scrubbing, dashboard exposure, or command-allowlisting boundaries.
  - Impact: future changes could widen trust boundaries silently.
  - Recommendation: add negative tests around env propagation and sensitive file handling.

## Fragile Areas
- High: several core modules are very large and mix UI, domain logic, and persistence.
  - Evidence: `src/listen/mod.rs` (2455 lines), `src/plan.rs` (2328), `src/technical.rs` (1873), `src/linear/transport.rs` (1450).
  - Impact: higher review cost, harder local reasoning, and more regression surface per change.
  - Recommendation: extract smaller state machines, rendering helpers, and side-effect adapters around the current public `run_*` entry points.
- Medium: listener state depends on many moving pieces at once: Linear state, local workspace clones, lock files, env variables, and workpad comments.
  - Impact: continuation/recovery bugs are more likely than in the simpler command families.
  - Recommendation: keep hard boundaries between claim logic, workspace provisioning, worker launch, and workpad sync.

## Technical Debt
- High: repo overlays are stale relative to the actual codebase.
  - Evidence: root `AGENTS.md` still references Elixir, `mix`, and `SymphonyMetaStack CLI.*`, while the repo is Rust/Cargo-based.
  - Impact: scan/agent prompts can inherit incorrect validation commands and conventions.
  - Recommendation: update or delete stale overlay guidance so agent context matches the live codebase.
- Medium: compatibility aliases and legacy layout handling remain throughout `src/lib.rs`, `src/scaffold.rs`, and tests.
  - Impact: they increase command-surface complexity and slow future cleanup.
  - Recommendation: keep a deprecation window, then remove alias-only paths and legacy directory migration code.
- Medium: logging is file-and-stdout based with no shared structured logger.
  - Impact: debugging long-running listener or cron failures requires manual log spelunking.
  - Recommendation: introduce a lightweight structured logging layer before adding more daemon behavior.

## Scaling Limits
- High: Linear payload sizes are fixed and partially hard-coded in `src/linear/transport.rs`.
  - Evidence: issues page size 100, labels 100, comments 50, attachments 100, teams 50.
  - Impact: heavily commented or attachment-heavy issues can be only partially mirrored locally.
  - Recommendation: page comments/attachments where needed or surface explicit truncation warnings.
- Medium: listener persistence is single-project JSON plus a single active lock in `src/listen/store.rs`.
  - Impact: it works well for one local host, but does not scale to multiple machines or shared runners coordinating the same repo.
  - Recommendation: keep it as a local-only design or move lock/state ownership to a remote coordinator.
- Medium: the local dashboard rebuilds whole HTML pages and full session views on each refresh in `src/listen/web.rs`.
  - Impact: fine for current sizes, but not ideal if session counts grow.
  - Recommendation: acceptable for now; revisit only if active session counts become large.

## Dependencies at Risk
- Medium: CI runs `rustfmt`, `clippy`, and tests, but no dependency audit or license policy gate is present.
  - Impact: vulnerable or risky crates can enter unnoticed.
  - Recommendation: add `cargo audit` and optionally `cargo deny` to CI.
- Low: release and install paths depend on external host tools (`curl`, `tar`, `cross`, `shasum`/`sha256sum`, `gh`) rather than a single hermetic build toolchain.
  - Impact: release portability depends on environment quality.
  - Recommendation: keep the scripts, but document tool prerequisites more aggressively and verify them in CI.

## Missing Critical Features
- High: `ReqwestLinearClient` in `src/linear/transport.rs` builds a default client with no explicit timeout or retry policy.
  - Impact: stalled HTTP calls can block interactive commands and unattended loops longer than intended.
  - Recommendation: set sane connect/request timeouts and add bounded retry/backoff for transient failures.
- Medium: no structured metrics/tracing exist for listener throughput, poll errors, or cron job latency.
  - Impact: regressions in unattended behavior are hard to detect before users notice.
  - Recommendation: add counters/timestamps to persisted state at minimum; tracing can come later.
- Medium: there is no built-in secret hygiene guard despite repo-local configs and attachment sync.
  - Impact: accidental credential commits or syncs can recur.
  - Recommendation: add a secret-scan check and warn when tracked files resemble exported tokens.

## Architectural Concerns
- High: command modules frequently combine parsing-mode decisions, prompt rendering, network calls, filesystem writes, and TUI rendering in one file.
  - Impact: architectural seams are weaker than the module layout suggests.
  - Recommendation: preserve current CLI behavior but keep extracting pure domain helpers from `src/plan.rs`, `src/technical.rs`, and `src/listen/mod.rs`.
- Medium: prompt/context assembly is string-template heavy across `src/workflows.rs`, `src/scan.rs`, and `src/workflow_contract.rs`.
  - Impact: prompt regressions are easy to introduce and hard to type-check.
  - Recommendation: centralize prompt section builders and add more output-shape assertions in tests.
- Medium: unattended sync safety relies partly on env markers like `METASTACK_LISTEN_UNATTENDED` in `src/sync_command.rs` and `src/linear/refine.rs`.
  - Impact: the boundary is effective but implicit.
  - Recommendation: move these checks behind a typed execution-context object instead of raw env lookups.

---
*Concerns audit: 2026-03-16*

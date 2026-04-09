# Agent Daemon

`meta listen` is the Symphony-inspired orchestration entrypoint for the Rust CLI. The current slice watches a Linear project, filters eligible tickets, claims newly eligible work, prepares an isolated clone-backed ticket workspace, downloads issue attachment context into that workspace, seeds the Linear workpad comment, launches a supervised worker that keeps running the configured local agent until the issue leaves the active workflow states, and surfaces progress in a dashboard without leaving the terminal. The listener now degrades instead of exiting on transient Linear failures and persists worker-side deferred Linear sync so local progress remains resumable after recovery.

## Design Goals

- Reuse the existing Linear client and `.metastack/` workspace instead of adding a one-off integration path.
- Keep daemon state install-scoped and inspectable so a small team can understand what the
  listener has done across projects.
- Provide a deterministic dashboard render path that works in local development, tests, and CI.
- Keep the runtime modular so later tickets can swap the placeholder pickup flow for real agent execution.

## First Slice

The initial implementation delivered in `MET-13` focuses on the smallest end-to-end loop:

1. `meta listen` polls Linear for Todo issues scoped by `--team` and optional `--project`.
2. Repo-scoped listen config can further require a specific issue label and/or require the assignee to match the Linear viewer tied to the API key.
3. Newly discovered eligible issues are moved to `In Progress`.
4. The daemon creates or refreshes a sibling `<repo>-workspace/<TICKET>` standalone clone rooted at `origin/main`, then checks out a deterministic ticket branch inside that clone.
5. The daemon downloads the issue's Linear attachments into `.metastack/agents/issue-context/<TICKET>/`, plus a generated `README.md` manifest describing downloaded files and any failures.
6. The daemon bootstraps or updates a single `## Codex Workpad` comment on the Linear issue.
7. The daemon writes an agent brief inside the workspace at `.metastack/agents/briefs/<TICKET>.md`.
8. The configured local agent is launched in the workspace with the issue context, attachment-context path, workpad comment id, and optional repo instructions file injected into its prompt/instructions.
9. Session state is persisted to the install-scoped MetaStack data root under
   `listen/projects/<PROJECT_KEY>/session.json`, per-session drill-down data is persisted under
   `listen/projects/<PROJECT_KEY>/session-details/<TICKET>.json`, and agent stdout/stderr are
   appended to `listen/projects/<PROJECT_KEY>/logs/<TICKET>.log`.
   Built-in provider-native manual resume metadata is stored as the same `{ provider, id }`
   record in both persisted artifacts. Historical repair intentionally keeps reading the current
   branded `--- intu listen turn ...` / `--- intu listen preflight failed @ ...` headers plus the
   legacy `--- meta ...` equivalents from those per-issue logs; preflight-failure blocks are valid
   persisted content, but they only delimit repair boundaries and do not themselves recover
   canonical provider/model/reasoning/tokens.
   Startup-critical listener JSON (`project.json`, `session.json`, and
   `active-listener.lock.json`) is written through same-directory temp-file persistence and loaded
   through deterministic sibling recovery (`.bak` before `.tmp`) when the primary file is corrupt.
   Successful recovery rewrites the primary file atomically, best-effort removes the consumed
   recovery artifact, optional detail artifacts remain best-effort, and stale or unreadable
   orphaned locks are removed with warnings while valid live locks keep blocking duplicate listener
   runs. Replacement-safe lock cleanup uses Unix file identity where available and falls back to
   best-effort path deletion on non-Unix hosts.
10. Session and workspace cleanup is two-tiered:
    - **Immediate auto-clean**: when a listener worker session completes (the ticket leaves active
      states), the worker attempts to remove the workspace clone and its ticket-scoped listen
      artifacts (session entry, detail, log) automatically. Auto-clean succeeds only when the
      workspace is clean (no uncommitted changes, no unpushed commits, HEAD not detached).
      When any safety check fails, the workspace is left in place and a manual-review skip is
      logged.
    - **Batch reconciliation**: `meta workspace prune` discovers previously missed merged
      workspaces across listener ticket clones, improve workspaces (`improve-<session-id>/`),
      and review remediation workspaces (`review-runs/pr-<number>/`), applies the same
      safety-first removal rules, and never deletes workspaces outside the expected sibling
      workspace roots.
    - Session records within `session.json` are removed or rewritten without deleting
      `project.json`, `active-listener.lock.json`, or unrelated per-issue logs, and live
      worker PIDs are never cleared automatically.
11. A full-screen ratatui dashboard renders runtime summary rows, a colorized agent table, the pending queue, daemon notes, and an active/completed session toggle.
12. The session table keeps a focused row selection, shows compact PR state (`none`, `draft #N`, `ready #N`), and opens a structured selected-session detail pane with `Enter`.
13. The hidden listen worker keeps refreshing the Linear issue and re-running the agent with first-turn and continuation prompts while the issue remains active.
14. Built-in provider turns and route-scoped E2E recipe steps now run under one shared subprocess supervisor that creates a dedicated Unix process group, applies the install-scoped listen turn timeout (default `1800s`), sends `SIGTERM`, waits the install-scoped graceful shutdown window (default `5s`), and escalates to `SIGKILL` only when the process group does not exit in time.
15. The hidden listen worker keeps looping while the issue remains active, but it treats repeated planning-only or no-op turns as a local stall instead of silently spinning. That stall logic is separate from timed-out subprocesses: a timeout means the active child process exceeded its runtime budget, while a stall means repeated completed turns made no meaningful progress.
16. Once the ticket branch is pushed, the worker creates or updates the matching branch PR as a draft, keeps the `metastack` label attached, and reuses the same PR on continuation instead of replacing it.
17. When the technical backlog is complete and meaningful non-`.metastack/` workspace progress was observed, the worker promotes that same branch PR to ready for review and then attempts to move both the parent issue and backlog child into a review-style state. If no matching open branch PR exists, the handoff keeps PR state at `none` and does not create a new PR during completion.
18. The worker records `completed` or `blocked` state locally, including timeout summaries (turn, elapsed time, timeout limit, PID, termination path), stall summaries, and recent agent log output for unattended failures.
19. During reconciliation, a stored listen-origin `running` session with a dead worker PID is
    classified through the shared blocked-taxonomy contract, records the latest stale-worker
    failure plus the automatic recovery attempt count, and is relaunched through the existing
    workspace/workpad context when the failure is retryable.
20. Automatic stale-worker recovery is capped at `2` attempts per operator-started session run.
    Missing workspace/workpad context, missing workspaces, paused sessions, execute-origin
    sessions, relaunch failures, and exhausted retry budgets stay blocked with structured reasons,
    while `R` / `meta listen sessions resume` continues to provide the manual retry path.
21. Completed sessions older than the default 24-hour TTL are pruned automatically during store
    loads and reconciliation, while blocked sessions are retained until explicit cleanup.
22. Live mode keeps the ratatui dashboard open in the terminal and uses the same shared listen snapshot for deterministic `--render-once` output.
23. Built-in `codex` and `claude` worker runs opportunistically capture structured input/output token usage when the provider surfaces it, accumulate those counts in the persisted session record across turns, append one explicit per-turn token summary line to the worker log after each completed turn, persist additive per-turn token history in `session-details/<TICKET>.json`, and leave token fields blank instead of failing when providers omit exact usage data.
24. Each listen run resolves one numeric context budget before any hidden worker spawn with the contract `--context-budget-tokens` override, then repo `.metastack/meta.json` `listen.context_budget_tokens`, then install `[defaults.listen].context_budget_tokens`, then built-in default `180000`. The worker derives `ContextPressure` from cumulative known input tokens on completed turns only, uses `pending_linear_sync.workpad_body` before the active workpad comment for one-time managed `#### Context Checkpoint` detection and preservation, clears the stored resume handle after a successful checkpoint turn, and renders the derived pressure in the selected-session detail pane without adding a new session-table column.

This mirrors the scheduler + status-surface split in Symphony while using one clear workspace
contract: each claimed ticket gets its own standalone clone and ticket branch under the configured
workspace root, while listener session state lives in a shared install-scoped store. The store key
is derived from the canonical source project root plus the effective project selector used for the
run, so the source repo checkout and any related worktrees still share one stored session per
project target while different project targets in the same checkout keep separate locks and logs.

Transient Linear failures during viewer refresh, issue listing, or reconciliation now preserve the
last known queue and session snapshot. The listener records degraded-state metadata in the shared
state file, keeps the dashboard and textual inspection surfaces populated, and schedules the next
retry using the shared install-scoped listen backoff. Worker-side Linear mutations that happen
after local progress exists, such as workpad sync, PR attachment, or review-state transitions, are
persisted as pending sync state and replayed on later `meta agents listen`, `meta agents execute`,
or `meta listen sessions resume` attempts.

## Command Surface

Primary options:

- `--team <KEY>`: Linear team scope.
- `--project <NAME|ID>`: optional project scope. Omitting it falls back to the repo default
  `linear.project_id` when configured.
- `--max-pickups <N>`: cap newly claimed issues per poll.
- `--poll-interval <SECONDS>`: refresh cadence for the live loop. Overrides the repo-scoped default when set.
- `--context-budget-tokens <TOKENS>`: override the listen known-input-token budget for this run and every hidden worker it launches. `meta listen sessions resume` exposes the same override for resumed sessions.
- `--once`: run a single live cycle and print a textual summary.
- `--render-once`: run a single cycle and print a deterministic ratatui snapshot.
- `--demo`: skip Linear and render sample queue/session data.
- repo-scoped and install-scoped context-budget persistence live on `meta runtime setup --listen-context-budget-tokens <TOKENS>` and `meta runtime config --listen-context-budget-tokens <TOKENS>`. Unset values fall back to `180000`.
- install-scoped listen retry backoff is configured through `meta runtime config
  --listen-retry-initial-backoff <SECONDS> --listen-retry-max-backoff <SECONDS>`. Unset values
  fall back to `2s` initial and `60s` max.
- install-scoped listen subprocess timeouts are configured through `meta runtime config
  --listen-agent-turn-timeout <SECONDS> --listen-agent-graceful-shutdown <SECONDS>`. Unset values
  fall back to `1800s` per agent turn and `5s` of graceful shutdown before escalation.
- install-scoped post-publication GitHub CI settle polling is configured through `meta runtime
  config --listen-ci-poll-interval <SECONDS> --listen-ci-poll-timeout <SECONDS>
  --listen-ci-timeout-behavior <block|warn-and-proceed>`. Unset values fall back to `30s`,
  `900s`, and `block`.
- `listen sessions list|inspect|clear|resume`: inspect or reuse stored project sessions from the
  install-scoped listener store. Use `--project` with `inspect`, `clear`, or `resume` to target a
  non-default project from the same checkout, or `--project-key` when you already know the stored
  install-scoped key.
- `listen sessions list` and `inspect` now show the latest tracked provider-native manual resume
  metadata for built-in `codex` and `claude` workers. The dashboard keeps only the compact handle,
  while these commands print the full latest resume ID and provider so operators can copy the
  correct resume target directly. Missing metadata is shown as explicitly unavailable instead of
  falling back to a legacy `session_id`.
- `listen sessions clear` accepts an issue identifier, `--blocked`, `--completed`, `--stale`, or
  `--all`; it refuses to remove any targeted record whose stored PID is still alive.
- `listen sessions list` and `inspect` surface the structured blocked taxonomy when it is present:
  blocked stages render as `Setup Err`, `Turn Err`, `Gate Err`, or `Infra Err`, while legacy
  sessions without blocked metadata stay on the generic `Blocked` fallback.
- Live dashboard keys: `Tab` toggles between panes or session views, `Left` cycles toward active
  sessions, `Right` cycles toward blocked and completed sessions, `Up` / `Down` move the selected
  row, `Enter` toggles the selected-session detail pane, `Esc` / `Backspace` close detail mode,
  `PgUp` / `PgDn` scroll the detail pane, `P` pauses the selected running worker, `R` resumes a
  paused worker or retries a blocked session, `Ctrl+Y` copies the focused pane with the shared
  export fallback, and `q` / `Ctrl-C` exits.

Examples:

```bash
meta agents listen --team MET
meta listen sessions list
meta agents listen --team MET --project "MetaStack CLI"
meta agents listen --team MET --project "MetaStack API"
meta listen sessions inspect --root . --project "MetaStack API"
meta listen sessions inspect --root . --project "MetaStack API" --turns
meta listen sessions clear --root . --project "MetaStack API"
meta listen sessions resume --root . --project "MetaStack API" --once
```

Repo-scoped listen settings in `.metastack/meta.json`:

- `listen.required_labels`: optional string list of labels; issues are eligible when any listed label matches case-insensitively.
- `listen.required_label`: legacy single-label compatibility input. New saves persist `required_labels`.
- `listen.assignment_scope`: `any`, `viewer_only`, or `viewer_or_unassigned`.
  - Legacy compatibility: existing `viewer` values still load as `viewer_or_unassigned`.
- `listen.refresh_policy`: `reuse_and_refresh` (default) or `recreate_from_origin_main`.
- `listen.instructions_path`: optional markdown file merged into the shared injected workflow contract for launched-agent instructions.
- `listen.poll_interval_seconds`: default Linear refresh cadence for `meta listen` when `--poll-interval` is not passed.

The shared failure classifier distinguishes transient, authentication, permission, configuration,
and other Linear failures. Transient failures drive degraded-state retries, while the non-transient
kinds remain visible as operator-actionable degraded state in runtime summaries and
`meta listen sessions inspect`.

Listen worker agent selection uses the shared built-in provider resolver:

1. explicit worker overrides such as `--agent`, `--model`, and `--reasoning`
2. the `agents.listen` command route override from `meta runtime config`
3. the `agents` route family override
4. repo defaults from `.metastack/meta.json`
5. install-scoped global defaults

When the selected provider is one of the built-in adapters, the listen worker also emits the
resolved provider/model/reasoning, route key, and config sources through the common launch
diagnostics and `METASTACK_AGENT_*` environment variables before the provider process starts.
Structured built-in output is also parsed for token telemetry so persisted listen sessions and the
dashboard can show cumulative `in`, `out`, and `total` counts when usage is available, while
unsupported or missing counts still render as `n/a`. The same capture result now also produces a
per-turn snapshot with `turn`, `prompt_mode`, and partial-or-complete token counts so the worker
log and `meta listen sessions inspect --turns` share one source of truth for turn-by-turn usage.
The selected-session detail pane derives `Context Pressure` from the selected session's
`turn_history` using the same shared pressure mapping the worker uses for checkpointing and
critical-turn limiting; there is no separate persisted pressure field. Workpad checkpoint
detection and preservation use the effective workpad body contract `pending_linear_sync.workpad_body`
first, active unresolved `## Codex Workpad` body second, so transient sync failures do not
duplicate the managed checkpoint block.
The built-in capture path is timeout-aware and no longer waits for provider stdout to reach EOF
before the worker can recover. Timeout summaries are mirrored into session detail, dashboard
detail, and textual inspect output as `Last timeout`, while repeated no-progress completed turns
continue to surface as stall summaries. Blocked sessions now also persist one additive blocked
contract with category, reason, and retryable status so session list rows, textual inspect output,
and the selected-session `Block Detail` pane stay aligned.
Listen-mode built-in launches also switch to machine-readable provider output so the worker can
capture the latest provider-native resume target for the current turn. Codex uses
`codex exec --json`, Claude uses `claude -p --verbose --output-format=stream-json`, and both
capture paths are silent best effort with no backfill of older stored session records. Built-in
worker restarts and `meta listen sessions resume` reuse the stored provider-native handle when it
matches the active built-in provider. Codex live token hydration follows the same rule by
resolving token files from the stored provider-native handle or the captured `thread.started`
event in the listen log, not from legacy continuation bookkeeping.
Structured session detail artifacts are best-effort companion state: malformed or missing
`session-details/<TICKET>.json` files do not break the list view or reload path, and the next
successful session refresh rewrites them. The detail artifact also stores the same
provider-native resume record used in `session.json`, so dashboard detail and textual inspection
render the same full manual resume target. The default inspect view stays compact; `--turns`
opts into rendering the persisted turn-history breakdown.

## Runtime Modules

- `src/listen/mod.rs`: command entrypoint, polling loop, shared snapshot model, state persistence, filtering, attachment-context download, workpad bootstrap, hidden listen worker flow, and prompt/instruction injection.
- `src/listen/dashboard.rs`: ratatui rendering for the live full-screen view and deterministic snapshots.
- `src/listen/workspace.rs`: clone-backed ticket workspace path, refresh, and branch preparation helpers.
- `src/listen/workpad.rs`: deterministic bootstrap workpad rendering.
- `src/agents.rs`: reusable brief-generation and agent-launch helpers shared by `meta listen`, `meta scan`, and the planning flows.
- `src/agent_provider.rs`: built-in provider adapter catalog and launch behavior for `codex` and `claude`.
- `src/workflow_contract.rs`: shared injected workflow contract composition plus optional repo overlay loading.
- `src/listen/store.rs`: install-scoped project identity, metadata, lock, session-store helpers,
  and per-session detail artifacts.

## Current Limitations

- Live mode runs in an alternate terminal screen, keeps list/detail navigation terminal-local, and exits on `q` or `Ctrl-C` without binding a local TCP port.
- Session persistence is install-scoped and local-file based; there is no remote coordination
  beyond the per-project active-listener lock yet.
- The supervised worker can mark a ticket `blocked` if it exhausts the configured turn cap, if a timed-out subprocess consumes the remaining turn budget, or if repeated turns fail to produce meaningful implementation updates while the issue stays active.
- Detail artifacts intentionally store only bounded milestones, references, PR metadata, and short
  log excerpts; raw log files remain the source of truth for full history.
- Agent rows already expose stage, age, local session handle, PID, PR state, and compact token
  totals, but real token/rate-limit telemetry is still limited until richer executor telemetry
  lands.

These are deliberate boundaries for the first slice. Future tickets can add agent executors, richer claim policies, and multi-agent coordination without replacing the command surface introduced here.

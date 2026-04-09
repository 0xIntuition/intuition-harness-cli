# Listen Phased Execution Spec

This document defines the phased execution model for `meta agents listen`.

## Goals

- Execute Linear tickets as quickly as possible with minimal orchestration overhead.
- Keep the Linear ticket as the primary work contract.
- Allow as many execution turns as needed until the ticket is complete.
- Preserve quality through explicit review phases instead of relying on backlog heuristics.
- Keep local backlog files and the Linear workpad synchronized as reporting artifacts, not as the completion gate.

## Source Of Truth

The Linear ticket is the execution source of truth:

- title
- description
- acceptance criteria
- validation / test-plan sections
- attachments and ticket discussion

Listener-specific CLI instructions remain additive and minimal. Local backlog files are supportive tracking artifacts only.

## Phase Model

Each listener cycle for an active ticket follows these phases:

1. `execute`
2. `review`
3. `continue` or `final_review`
4. `verify`
5. `validate`
6. `publish`

### Execute

The execution phase receives:

- ticket context from Linear
- workspace path
- minimal repo/listener contract
- attachment context paths when present
- current workpad reference
- compact delta context from the previous review on continuation turns

The execution phase must attempt to complete as much of the remaining ticket work as possible before stopping.

Before the first worker turn, the command path resolves one numeric listen context budget with
this precedence:

- `meta agents listen --context-budget-tokens <TOKENS>` or `meta listen sessions resume --context-budget-tokens <TOKENS>`
- repo `.metastack/meta.json` `listen.context_budget_tokens`
- install `[defaults.listen].context_budget_tokens`
- built-in default `180000`

The worker derives `ContextPressure` from cumulative known input tokens across completed turns
only:

- `normal`: usage `< 70%`
- `elevated`: usage `>= 70%` and `< 85%`
- `high`: usage `>= 85%` and `< 95%`
- `critical`: usage `>= 95%`

Pressure changes execution behavior without creating a new persisted pressure field:

- `elevated` adds a warning plus concise progress-capture hint to continuation prompts only
- `high` triggers one dedicated checkpoint turn when the effective workpad body does not yet carry
  the managed `#### Context Checkpoint` block
- `critical` keeps that one-time checkpoint behavior, injects a wrap-up directive into the active
  execution prompt branch, and caps only the remaining execution-turn budget to one

Each execution turn is also bounded by the shared listen subprocess supervisor:

- install-scoped `defaults.listen.agent_turn_timeout_seconds` defaults to `1800`
- install-scoped `defaults.listen.agent_graceful_shutdown_seconds` defaults to `5`
- built-in provider turns run in their own Unix process group
- timeout expiry sends `SIGTERM` first, waits the graceful-shutdown window, then escalates to `SIGKILL` only if the process group is still alive

Timed-out subprocesses are reported separately from stalled turns. A timeout means the active child
process exceeded its runtime budget; a stall still means repeated completed turns finished without
meaningful progress.

### Review

The review phase compares the current workspace against:

- Linear acceptance criteria
- explicit validation requirements
- the requested ticket deliverables

The review phase must return structured JSON with:

- `summary`
- `complete`
- `completed_items`
- `remaining_items`
- `validation_completed`
- `validation_remaining`
- `risks`
- `notes`

The listener uses this review output to:

- update the Linear workpad comment
- update the local backlog progress checklist section
- decide whether another execution turn is required

### Continue

If the review phase reports incomplete work, the next execution turn receives compact delta context only:

- what was completed
- what remains
- validation still required
- risks that still need attention

This continuation path avoids re-injecting the full ticket context when it is not necessary. When
pressure is `elevated`, this is the only prompt branch that receives the pressure warning.

### Final Review

When the review phase reports `complete = true`, the listener runs one more fast safety review. The final review must return structured JSON with:

- `approved`
- `summary`
- `missing_items`
- `validation_gaps`
- `risks`
- `notes`

If final review fails, the missing items become the next continuation delta and execution resumes.

### Verify

When final review approves the work, the listener runs a dedicated verification phase before any
local validation or PR mutation.

Verification resolves the dedicated `agents.listen.verification` route through the same
provider/model/reasoning precedence and launch-diagnostics path used by other agent-backed
commands. The verification pass combines:

- built-in quality criteria
- install-scoped `[verification]` criteria extensions and booleans
- route-scoped recipe overrides from `<project-dir>/verification/recipes/agents.listen.yaml`
- deterministic battle-test inputs from `<project-dir>/verification/inputs/agents.listen/`

Verification persists one structured JSON report plus a markdown mirror alongside the other listen
artifacts. The latest compact verification summary is mirrored into inspect output, dashboard
detail, PR body rendering, and workpad reporting.

The verification report includes:

- overall status and summary
- resolved verification-route diagnostics
- per-criterion code-review results with findings and remediation
- route-scoped E2E step results with bounded runtime plus bounded stdout/stderr evidence
- aggregate battle-test sampling results
- remediation items for the next repair turn

If the verifier output is missing or malformed, verification fails closed instead of silently
approving the ticket. Verification failures use their own bounded retry budget: the draft PR stays
in place, remediation is injected into the next execution turn, and the worker blocks when the
verification repair budget is exhausted.

Route-scoped E2E recipe steps accept an optional `timeout_seconds`. Omitted values default to
`300`. When a step times out, verification records that step as timed out with its elapsed/runtime
budget details instead of leaving the worker hung or collapsing the result into stalled-turn
reporting.

### Validate

Before any PR create, PR edit, or ready-promotion mutation, the listener resolves one shared local
validation profile with this precedence:

- CLI override when a command path exposes one
- repo-scoped `.metastack/meta.json` `validation.commands`
- built-in repository heuristics (`make quality`, then `make all`, then `cargo test`)

The resolved profile includes source diagnostics plus an optional repo-scoped profile label. The
listener writes the active profile to worker logs, surfaces it in `meta agents listen --check`,
and records `Validating` as an explicit session phase in persisted state and dashboard summaries.

If local validation fails:

- stdout/stderr excerpts are captured in the worker log
- the review delta is rewritten with concise repair context
- the workpad and local backlog progress mirror that repair context
- a dedicated repo-scoped repair-turn budget is decremented before the worker re-enters `execute`
- PR mutation is blocked when the validation repair budget is exhausted

### Publish

When final review approves the work:

- the branch PR is refreshed and promoted to ready
- the `metastack` label is preserved
- the PR is attached to Linear
- the Linear ticket is moved from `In Progress` to the review-style state

The same validation gate also runs before draft PR publication or draft PR refreshes on
continuation turns.

After draft publication and again before ready promotion, the listener runs a bounded
post-publication settle poll against the active branch PR using `gh pr checks --json
name,state,bucket,description,link`. The poll classifies checks as pending, passed, failed, or
absent (`no checks configured`) and records the current settle state in session summaries and
milestones.

If CI is still pending:

- the session stays in `Publishing`
- the summary, inspect output, and dashboard detail show explicit waiting progress plus remaining
  timeout budget
- polling sleeps for the configured install-scoped interval before checking again

If CI is red:

- the same PR is kept in place instead of creating a duplicate
- concise failing-check details are added to the next continuation delta
- the worker re-enters `execute`
- local validation runs again before the next PR mutation

If no checks are configured:

- the worker records that state explicitly
- ready handoff proceeds without inventing a synthetic pass/fail result

If the settle timeout is reached:

- `ci_timeout_behavior = "block"` blocks review handoff with a gate error
- `ci_timeout_behavior = "warn_and_proceed"` records a warning summary and continues
- the same repair-turn budget gates additional retries

## Linear Failure Recovery

Listen-family Linear recovery now uses one shared classifier and one install-scoped retry
contract:

- Transient failures during viewer refresh, issue listing, or reconciliation do not terminate the
  daemon. The listener preserves the last known queue and session snapshot, records degraded-state
  metadata, and schedules the next retry from `[defaults.listen.retry]`.
- Authentication, permission, and configuration failures remain visible as degraded
  operator-actionable state instead of being treated as retryable transient outages.
- If a worker already created local workspace progress, later Linear failures in issue refresh,
  workpad sync, PR attachment, or review-state transitions are persisted as `pending_linear_sync`
  and replayed on later `meta agents listen`, `meta agents execute`, or
  `meta listen sessions resume` runs.
- Successful replay clears the pending sync state from the persisted session artifacts.

## Tracking Artifacts

### Linear Workpad

The active `## Codex Workpad` comment is rewritten after each review phase to show:

- summary
- completed checklist
- remaining checklist
- validation checklist
- risks / notes

The effective workpad body is `pending_linear_sync.workpad_body` when present and otherwise the
active unresolved `## Codex Workpad` comment body from Linear. One managed `#### Context
Checkpoint` block lives under `### Review Notes`, is detected from that effective body, and is
preserved through later review-workpad rewrites so transient sync failures do not duplicate it.
That checkpoint records the current pressure state, turns completed, known input tokens,
completed/remaining/validation checklist state, and current workspace status.

### Verification Reports

Each verification pass writes:

- a JSON report under the listen store verification directory
- a markdown report under the same verification directory
- a compact summary mirrored into the session detail artifact

### Local Backlog

If a local backlog entry exists, the listener updates a managed section in `index.md`:

- `## Listener Progress Checklist`

This section is reporting output only. It must not decide ticket completion by itself.

### Persisted Listener State

The install-scoped listen store also carries:

- atomic same-directory temp-file persistence for startup-critical JSON so failed writes leave the
  previously readable primary file intact
- deterministic recovery for required `project.json`, `session.json`, and
  `active-listener.lock.json` loads, preferring `.bak` artifacts before `.tmp` siblings,
  rewriting the recovered primary file atomically, and best-effort removing the consumed recovery
  artifact afterward
- replacement-safe active lock cleanup that uses Unix file identity where available and falls back
  to best-effort path deletion on non-Unix hosts
- degraded Linear state, including failure kind, failure message, and retry timing
- deferred worker-side `pending_linear_sync` metadata for replayable remote operations
- optional blocked-session taxonomy metadata with category, reason, and retryable status so
  blocked rows, textual inspect output, and selected-session detail all render from one contract
- the latest timeout snapshot for a timed-out worker turn, including turn number, elapsed time,
  timeout limit, PID, graceful-shutdown window, and final termination path

Context pressure itself is not persisted. The selected-session detail pane derives `Context
Pressure` from the selected `AgentSession.turn_history` using the shared pressure mapping.

## Completion Rules

The listener no longer treats backlog completeness as the completion gate.

A ticket is complete only when:

- review says the ticket deliverables are complete
- final review approves the result
- verification passes
- publish / Linear review handoff succeeds

## Turn Budget

`max_turns` limits execution turns, not the lightweight review/final-review/verification passes
around each turn.

Timed-out subprocesses still consume execution turns. The listener records the timeout, feeds the
next continuation turn through the existing retry/repair path when budget remains, and blocks with
timeout-specific reporting when the turn budget is exhausted.

Under `critical` context pressure, only the remaining execution-turn budget is reduced to one. The
review, final-review, verify, validate, and publish phases still execute normally after that final
execution turn completes.

This preserves quality while keeping the main execution loop bounded.

# Intuition Engineer CLI — Repository Specification

## OVERVIEW

The Intuition CLI (`intu`) is a Rust terminal tool that unifies repository-scoped planning, Linear project management, codebase context generation, and agent-backed automation into a single command-line surface. It is designed for engineering teams that want planning state, issue tracking, and unattended agent execution to live close to the code rather than scattered across disconnected tools.

The CLI manages a per-repository `.intuition/` workspace that stores specs, backlog packets, codebase context, workflow playbooks, cron job definitions, and agent session state. All persistent project configuration flows through `.intuition/meta.json` (repo-scoped) and a TOML config file (install-scoped), with a consistent precedence model: CLI flag > route override > repo default > install default > built-in fallback.

The binary ships as `intu`, is built with Rust 1.85+ (edition 2024), and targets macOS and Linux. It embeds agent workflow contracts, backlog templates, review instructions, and workflow playbooks at compile time via `include_str!`.

## GOALS

1. **Single entry point for planning and execution.** Engineers should be able to create specs, generate backlog tickets, scan codebase context, run agents, merge PRs, review shipped work, and manage workspaces without leaving the terminal or switching between tools.

2. **Linear-native workflow integration.** The CLI treats Linear as the authoritative issue tracker. It reads and writes issues, syncs local backlog packets with pull/push operations, resolves team/project/state metadata from Linear, and drives issue state transitions during agent-backed execution.

3. **Repository-scoped state under `.intuition/`.** All planning artifacts, agent session logs, backlog packets, codebase context, workflow definitions, cron jobs, and merge-run audit trails live under a single directory in the repository, making them versionable, portable, and auditable.

4. **Unattended agent orchestration.** `intu agents listen` polls Linear for eligible issues, creates isolated workspace clones, and runs coding agents (Codex or Claude) end-to-end against those issues with a phased execution loop (execute turn, review, continuation, final review, PR publication). `intu agents execute` provides single-issue headless runs whose sessions are adoptable by the listener.

5. **Safe workspace isolation.** Agent work always happens in dedicated sibling clones under `<repo>-workspace/`, never in the developer's source checkout. Listener workspaces auto-clean on session completion when safe, and `intu workspace prune` reconciles stale clones across all workspace families (listener, improve, review).

6. **Consistent, layered configuration.** Install-scoped config (Linear auth/profiles, agent defaults, routing, merge knobs, UI preferences) and repo-scoped config (team, project, provider overrides, listen settings) compose predictably. Advanced per-family and per-command agent routing lets teams tune provider/model/reasoning without per-run flags.

7. **Interactive TUI-first experience.** Dashboards, editors, and selection flows use ratatui-based terminal UIs with shared scrolling, markdown rendering, copy/export (`Ctrl+Y` with clipboard fallback to terminal export overlay), optional vim-mode navigation, and search-first issue browsers. Non-interactive (`--no-interactive`), JSON (`--json`), and render-once (`--render-once`) output modes exist for scripting, CI, and snapshot testing.

8. **Verified self-update.** Release installs on macOS/Linux can self-update via `intu upgrade` with SHA256 verification against published checksums, dry-run previews, version pinning, prerelease opt-in, and downgrade protection.

## FEATURES

### Backlog management (`intu backlog`)

- **`backlog spec`** — Create or iteratively improve the repo-local `.intuition/SPEC.md` through a staged agent-backed interview. On first run, asks what the repository should build and drafts the spec. On later runs, loads the existing spec and revises it based on requested changes. Generated markdown must include `OVERVIEW`, `GOALS`, `FEATURES`, and `NON-GOALS` headings.
- **`backlog plan`** — Turn a planning request into one or more Linear backlog issues. Supports interactive follow-up questions, configurable ticket defaults (assignee, state, priority, labels, project), fast single-pass mode (`--fast`), multi-ticket output (`--multi`), follow-up caps (`--questions`), and run-scoped provider continuation. Reshape mode (`intu backlog plan <IDENTIFIER>`) rewrites an existing issue in place with before/after diff preview. Zero-prompt mode resolves defaults through remembered project/team, velocity defaults, repo config, global config, then built-in behavior.
- **`backlog improve`** — Scan repo-scoped backlog issues for hygiene gaps. Classifies each as `no_update_needed`, `ready_for_update`, `needs_planning`, or `needs_questions`. Stays inside one guided dashboard flow. Requires explicit human accept/reject before any mutation. Supports `basic` (metadata hygiene) and `advanced` (content rewrite, parent-issue proposals) modes. Writes immutable artifacts under `.intuition/backlog/<ISSUE>/artifacts/improvement/<RUN_ID>/`.
- **`backlog tech`** (aliases: `split`, `derive`) — Create a technical sub-issue and local planning packet from a parent Linear issue. Child tickets inherit the parent's workflow status by default. Generates the full `.intuition/backlog/<ISSUE>/` file tree from the embedded template. Interactive parent-issue picker with shared search browser.
- **`backlog sync`** — Pull and push operations to keep local `.intuition/backlog/` packets aligned with Linear issue state. Interactive sync dashboard with search-first issue browser and per-entry sync state (`synced`, `local-ahead`, `remote-ahead`, `diverged`, `unlinked`). Pull downloads issue descriptions, attachment files, ticket images with localized paths, and discussion context. Push upserts checklist completion comments and optionally updates descriptions with conflict guards.

### Linear integration (`intu linear`)

- **`linear issues`** — List, create, edit, and browse Linear issues with team/project/state filters. Shared free-text search ranks exact identifiers first, then prefixes and token matches. Supports `--json` output, `--no-interactive` for scripted mutations, and `--render-once` for TUI snapshots.
- **`linear issues refine`** — Quality-improvement rewrite passes on existing issues. Critique-only by default; `--apply` writes immutable refinement artifacts under `.intuition/backlog/<ISSUE>/artifacts/refinement/<RUN_ID>/` before mutating Linear. Blocked during `intu listen` for the active ticket.
- **`linear projects`** — Browse and select Linear projects by team.

### Agent orchestration (`intu agents`)

- **`agents listen`** — Long-running daemon that polls Linear for eligible issues (filtered by team, project, label, assignee scope), creates isolated workspace clones, and runs agent turns via the configured provider. Follows an explicit phased execution loop: execution turn, review against acceptance criteria, continuation turns with remaining-work delta, final review, then PR publication and Linear state transition. Manages retry, reconciliation, auto-cleanup, and per-session token telemetry. Interactive dual-pane dashboard (Agent Sessions + In Progress Issues) with `P` to pause a running worker, `R` to resume a paused or retry a blocked session. Supports `--check` preflight, `--once` single-poll, `--all-assignees` scope override, and `--hide-active-issues`/`--hide-preview` layout toggles.
- **`agents listen sessions`** — Install-scoped session management for the listen daemon:
  - `list` — List stored project sessions.
  - `inspect` — Inspect a stored session, with `--turns` for per-turn token history.
  - `clear` — Clear selected sessions (by issue identifier, `--blocked`, `--completed`, `--stale`, or `--all`).
  - `resume` — Resume listening for a stored session (supports `--once`).
- **`agents execute`** — One-off headless agent run for a single Linear issue. Persists session state adoptable by `listen` (visible with `execute-origin` label, not auto-claimed).
- **`agents review`** — Guided dashboard for auditing GitHub PRs. Direct review (single PR) or guided queue mode (`metastack`-labeled PRs). Full lifecycle: Selected → Review In Progress → Review Complete → Fix Agent Pending/Running/Complete or Skipped. Can open remediation PRs from isolated workspaces. Supports `--fix-pr` and `--skip-pr` for scripted usage.
- **`agents retro`** — Analyze merged and open PRs for follow-up backlog opportunities. Guided retro dashboard with filter panel (`F`) supporting state, author, label, and assignee filters. After analysis, opens a plan-style ticket curation flow for creating curated batches in Linear.
- **`agents improve`** — Inspect open PRs, accept improvement instructions, and publish stacked PRs targeting the source PR branch from isolated `improve-<session-id>/` workspaces. Dual-pane TUI with PR list and persistent session list.
- **`agents workflows`** — List, explain, and run reusable workflow playbooks (built-in and repo-local under `.intuition/workflows/`). TUI-first on interactive TTY with guided wizard, review/export dashboard, inline edit (`e`), and save (`s`). Deterministic fallback with `--no-interactive --param key=value`. Render-once mode with `--events` for scripted snapshot walkthroughs.

### Codebase context (`intu context`)

- **`context scan`** — Analyze the repository and produce structured context files under `.intuition/codebase/`: `SCAN.md`, `ARCHITECTURE.md`, `CONCERNS.md`, `CONVENTIONS.md`, `INTEGRATIONS.md`, `STACK.md`, `STRUCTURE.md`, `TESTING.md`. Progress dashboard on TTY; agent output captured in `.intuition/agents/sessions/scan.log`. Supports `--json` for machine output.
- **`context show`** — Display the current effective agent context (repo-scoped instructions, loaded rules, codebase context sources).
- **`context map`** — Print a repo-map style summary from the live repository tree.
- **`context doctor`** — Report missing or stale context inputs (meta.json, repo rules, instructions, generated docs).
- **`context reload`** — Re-run the context refresh path used by scan.

### Configuration (`intu runtime`)

- **`runtime config`** — Install-scoped config dashboard. Manages Linear auth and named profiles, agent defaults (provider/model/reasoning), backlog ticket defaults (assignee, state, priority, labels, velocity defaults), merge retry knobs, vim mode, and advanced per-family/per-command agent routing. Includes shared onboarding wizard for fresh installs (`--replay-onboarding` to rerun). Reasoning is validated against the selected provider/model catalog.
- **`runtime setup`** — Repo-scoped setup. Scaffolds `.intuition/` directory tree, saves repo defaults to `meta.json`, validates provider/model/reasoning combinations against the install catalog, resolves project names to Linear project IDs, and seeds backlog templates from embedded artifacts. Safe to rerun. Prompts for conflict resolution on changed template files in interactive mode.
- **`runtime cron`** — Create, validate, and manage repository-local cron jobs as Markdown with YAML front matter. Supports single-step legacy jobs and multi-step workflow mode with durable steps (`shell`, `agent`, `cli`, `approval`), retry policies with backoff, approval checkpoints, and conditional branching (`when` clauses). Persists run state under `.intuition/cron/.runtime/`. Commands: `init`, `list`, `validate`, `status`, `start`, `stop`, `run`, `approvals`, `approve`, `reject`, `resume`. The `start` command launches a detached scheduler daemon; `status` reports scheduler health plus known jobs; `stop` terminates the scheduler.

### Merge batching (`intu merge`)

- Discover open GitHub PRs, select a batch in a one-shot TUI dashboard, run an aggregate merge in an isolated workspace, validate with configurable commands (defaulting to `make quality` > `make all` > `cargo test`), and create/update one aggregate PR. Supports `--resume-run` for reusing existing aggregate branches, `--no-interactive` for scripted execution, and bounded retry with install-scoped knobs (`validation_repair_attempts`, `validation_transient_retry_attempts`, `publication_retry_attempts`). Validation narrows repeated failures before rerunning the full suite. Validation is not a hard publication gate — unresolved status is recorded in the PR body. Writes structured audit artifacts under `.intuition/merge-runs/<RUN_ID>/`.

### Workspace management (`intu workspace`)

- **`workspace list`** — Inventory sibling workspace clones with git safety signals and Linear/PR status.
- **`workspace clean`** — Remove individual clones or `target/` directories. Also removes associated ticket-scoped listen artifacts.
- **`workspace prune`** — Batch reconciliation across all managed workspace families: listener clones (removed when Linear ticket is Done/Cancelled and workspace is safe), improve workspaces (removed when associated PR is merged/closed), and review workspaces. Reports reclaimed-space summaries.

### Dashboards (`intu dashboard`)

- **`dashboard linear`** — Linear work dashboard with shared search and markdown preview.
- **`dashboard agents`** — Agent session dashboard.
- **`dashboard team`** — Team review dashboard by team and project.
- **`dashboard ops`** — Operational overview dashboard.

### Self-update (`intu upgrade`)

- Check, dry-run, and apply GitHub Release self-updates with SHA256 verification against published `SHA256SUMS`, version pinning, prerelease opt-in, and deliberate downgrade support. Refuses unsafe origins (Cargo installs, source checkouts).

### Shared TUI contract

- **Copy/export** — `Ctrl+Y` copies the focused field or pane as plain text with markdown source preservation. When the local clipboard is unavailable, a terminal-safe export overlay opens instead of failing silently. Implemented in `src/tui/copy.rs` and applied across all interactive surfaces.
- **Markdown rendering** — All TUI preview and detail panes that display markdown-authored content use the shared `src/tui/markdown.rs` renderer. Per-surface markdown rendering helpers are not permitted.
- **Scrolling** — Long-form editors and preview panes share one scrolling model: `Up/Down/PgUp/PgDn/Home/End` move within wrapped content, and mouse-wheel scrolling applies to the focused pane.
- **Vim mode** — When enabled via `runtime config --vim-mode enabled`, TUIs add `h/j/k/l` aliases only on non-text-focused controls. Search bars and text editors continue to insert literal characters.

### Machine-readable contract

All mutation commands support `--no-interactive` for promptless scripted runs (implying JSON output). Read-only and headless flows support `--json`. `--render-once` provides terminal snapshots for humans and snapshot-style tests. Machine failures use a stable envelope: `status`, `command`, `error { code, message, context? }`.

## NON-GOALS

1. **Web UI or hosted service.** The CLI is terminal-native. It does not serve HTTP endpoints, host dashboards in a browser, or run as a persistent web application.

2. **Direct code generation or IDE integration.** The CLI orchestrates external coding agents (Codex, Claude) but does not generate application code itself. It does not provide LSP, editor plugins, or IDE extensions.

3. **Multi-repository orchestration from a single command.** Each `intu` invocation is scoped to one repository root. Cross-repo coordination is not a goal; each repository manages its own `.intuition/` state independently.

4. **Replacing Linear as the issue tracker.** The CLI complements Linear; it does not replicate Linear's UI, notification system, or collaboration features. Issue browsing and mutation always resolve through the Linear GraphQL API. The CLI is read-only for workflow state creation — new states must be created in the Linear UI first.

5. **General-purpose CI/CD pipeline.** While `intu merge` runs validation and `intu agents listen` drives agent execution, the CLI is not a build system or deployment pipeline. It delegates to existing project tooling (`make`, `cargo`, etc.) for validation.

6. **Windows support.** The binary and install scripts target macOS and Linux only.

7. **Custom agent provider plugins.** The built-in provider catalog covers Codex and Claude with validated reasoning levels. The CLI does not expose a plugin API for arbitrary third-party agent providers.

8. **Persistent cross-run agent sessions.** Run-scoped provider continuation handles may be reused within a single command run but are never persisted under `.intuition/`, shared with other commands, or coupled to listen-worker session state.

## ARCHITECTURE NOTES

- **Binary name:** `intu` (configured via `[package.metadata.branding]` in `Cargo.toml`).
- **Project directory:** `.intuition/` (branding-driven, was `.metastack/`).
- **Config resolution:** CLI flag > command route override > family route override > repo `meta.json` > install TOML > built-in default.
- **Agent workflow contract:** Embedded at compile time from `src/artifacts/injected-agent-workflow-contract.md`.
- **Backlog templates:** Embedded from `src/artifacts/BACKLOG_TEMPLATE/`.
- **Review instructions:** Embedded from `src/artifacts/REVIEW.md` and `src/artifacts/VIEW_LINEAR.md`.
- **Workflow playbooks:** Built-in from `src/artifacts/workflows/`, repo-local from `.intuition/workflows/`.
- **Cron examples:** Shipped disabled-by-default from `src/artifacts/cron/`.
- **TUI framework:** ratatui 0.30 + crossterm 0.29, with shared markdown rendering (`src/tui/markdown.rs`) and copy/export contract (`src/tui/copy.rs`).
- **Async runtime:** tokio multi-threaded.
- **Error handling:** `anyhow::Result` with `.context()` throughout; no `unwrap()`/`expect()` in production code.
- **Quality gate:** `make quality` (fmt check + clippy with `-D warnings` + tests).
- **Install-scoped data:** Listener state, session details, and logs stored under the config-derived data root (e.g. `~/.config/metastack/data/listen/projects/<PROJECT_KEY>/`).

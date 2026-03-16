# Architecture
**Analysis Date:** 2026-03-16

## Pattern Overview
- Single-binary Rust CLI centered on `src/main.rs`, `src/lib.rs`, and `src/cli.rs`.
- Modular command-family architecture: backlog, Linear, context/scan, runtime/setup/cron, agents/listen, and dashboards each live in separate `src/*.rs` or `src/*/` modules.
- Filesystem-first design: repo state lives under `.metastack/`, while install-scoped listener state lives beside the resolved config path via `src/config.rs` and `src/listen/store.rs`.
- Service-adapter split for Linear: `src/linear/service.rs` owns domain rules; `src/linear/transport.rs` owns GraphQL I/O.
- Local-agent orchestration is process-based, not SDK-based: prompts are rendered in Rust and executed through external binaries from `src/agents/execution.rs`.
- No database, ORM, background queue service, or public HTTP API server is present.

## Layers
- CLI surface:
  - `src/main.rs` starts Tokio and exits non-zero on failure.
  - `src/lib.rs` parses CLI args, prints compatibility hints, and dispatches to command modules.
  - `src/cli.rs` defines the public command tree and shared arg structs.
- Application/workflow layer:
  - `src/setup.rs`, `src/scaffold.rs`, `src/scan.rs`, `src/context.rs` manage repo bootstrap and generated context.
  - `src/plan.rs`, `src/technical.rs`, `src/sync_command.rs`, `src/backlog.rs` manage backlog planning, templates, and Linear-file sync.
  - `src/workflows.rs` renders reusable workflow playbooks from `workflows/builtin/` and `.metastack/workflows/`.
  - `src/listen/mod.rs`, `src/listen/worker.rs`, `src/listen/workpad.rs`, `src/listen/workspace.rs` implement unattended ticket execution.
  - `src/cron.rs` and `src/cron/runtime.rs` implement repo-local scheduled jobs.
- Integration layer:
  - `src/linear/service.rs` resolves teams, projects, labels, workpad comments, and issue filtering.
  - `src/linear/transport.rs` performs GraphQL requests, uploads, downloads, and attachment mutations with `reqwest`.
  - `src/agents/execution.rs` resolves configured agent commands and spawns local processes.
  - `src/listen/web.rs` exposes a loopback-only dashboard for listener state.
- Persistence layer:
  - `src/fs.rs` centralizes `.metastack/` paths and file writes.
  - `src/backlog.rs` renders and stores Markdown backlog artifacts plus `.linear.json`.
  - `src/listen/store.rs` persists listener metadata, session state, logs, and process locks under the install-scoped data root.
- Presentation layer:
  - `src/*dashboard.rs` and `src/tui/fields.rs` provide terminal dashboards and interactive forms using `ratatui` and `crossterm`.
  - `src/listen/web.rs` mirrors listener data as HTML for a browser view.

## Data Flow
- Repo setup and scan:
  - `meta runtime setup` in `src/setup.rs` scaffolds `.metastack/`, saves repo defaults to `.metastack/meta.json`, and can create default Linear labels.
  - `meta context scan` in `src/scan.rs` collects repo facts, writes `.metastack/codebase/SCAN.md`, then invokes the configured local agent to author the higher-level docs.
- Backlog planning:
  - `meta backlog plan` in `src/plan.rs` loads repo defaults, asks an agent for follow-up questions and issue drafts, creates Linear backlog issues, then materializes matching local backlog folders from `tmp/_TEMPLATE/`.
  - `meta backlog split` in `src/technical.rs` loads a parent issue, extracts acceptance criteria, generates a child backlog draft, creates the Linear child issue, writes local files, then pushes them back through `meta backlog sync push`.
- Sync lifecycle:
  - `meta backlog sync pull` in `src/sync_command.rs` fetches the Linear issue, writes `index.md`, restores managed attachment files, and updates `.linear.json`.
  - `meta backlog sync push` reads local files, updates the Linear description from `index.md`, reuploads managed attachments, and rewrites `.linear.json`.
- Listener lifecycle:
  - `meta agents listen` in `src/listen/mod.rs` loads repo/install config, polls Linear, claims Todo issues, provisions a dedicated workspace clone via `src/listen/workspace.rs`, writes a workpad comment, spawns an agent worker, and persists state/logs through `src/listen/store.rs`.
  - The worker injects issue/workspace env vars in `src/listen/worker.rs`, runs the local agent, and the parent loop updates dashboard state plus the browser/TUI views.
- Cron lifecycle:
  - `meta runtime cron init` writes Markdown job definitions under `.metastack/cron/`.
  - `meta runtime cron start|daemon|run` in `src/cron/runtime.rs` parses job front matter, executes commands or prompts, records runtime state under `.metastack/cron/.runtime/`, and emits per-job logs.

## Key Abstractions
- `PlanningPaths` in `src/fs.rs`: canonical builder for `.metastack/` paths such as `SCAN.md`, backlog roots, cron runtime paths, and agent session logs.
- `AppConfig` and `PlanningMeta` in `src/config.rs`: split install-scoped config from repo-scoped defaults.
- `RepoTarget` in `src/repo_target.rs`: derives project identity and renders the repo-scope block injected into agent prompts.
- `WorkflowInstructionBundle` in `src/workflow_contract.rs`: merges built-in workflow contract text with `AGENTS.md`, legacy `WORKFLOW.md`, and optional repo-scoped instructions.
- `LinearService<C>` in `src/linear/service.rs`: stable domain API over a pluggable `LinearClient`.
- `ReqwestLinearClient` in `src/linear/transport.rs`: concrete GraphQL/file transport.
- `TicketWorkspace` in `src/listen/workspace.rs`: describes the listener-managed workspace clone, branch, base ref, and provisioning mode.
- `ListenProjectStore` in `src/listen/store.rs`: keyed state/log/lock store for one canonical source repo.

## Entry Points
- Binary:
  - `src/main.rs`
- Library dispatch:
  - `src/lib.rs`
- Primary command definitions:
  - `src/cli.rs`
- Repo bootstrap:
  - `src/setup.rs`
  - `src/scaffold.rs`
- Repo scan and context:
  - `src/scan.rs`
  - `src/context.rs`
- Linear-facing commands:
  - `src/linear/command.rs`
- Unattended execution:
  - `src/listen/mod.rs`
  - `src/listen/worker.rs`
- Scheduled execution:
  - `src/cron.rs`
  - `src/cron/runtime.rs`
- Shell entry points:
  - `scripts/install-meta.sh`
  - `scripts/release-artifacts.sh`

## Error Handling
- `anyhow::Result` is the default error boundary across command modules; failures are enriched with `Context` close to I/O or process boundaries.
- Validation is mostly early and user-facing:
  - missing config or auth in `src/config.rs`
  - missing `.metastack/meta.json` via `load_required_planning_meta`
  - non-interactive argument requirements in `src/plan.rs`, `src/technical.rs`, and `src/cron.rs`
  - workspace safety checks in `src/listen/workspace.rs`
- Linear transport decodes GraphQL `errors` arrays and non-2xx responses in `src/linear/transport.rs`.
- Fallback behavior exists for some views:
  - non-TTY commands fall back to text mode
  - `src/listen/web.rs` renders a fallback dashboard when state reads fail
  - `src/context.rs` tolerates missing optional overlays/instructions.
- Retries/backoff are largely absent; most network/process failures bubble straight to the caller.

## Cross-Cutting Concerns
- Configuration:
  - install-scoped TOML config via `$METASTACK_CONFIG` or XDG/HOME paths in `src/config.rs`
  - repo-scoped JSON defaults in `.metastack/meta.json`
- Workspace safety:
  - listener work happens in sibling workspace clones from `src/listen/workspace.rs`, not the source checkout.
- Logging and audit trail:
  - user-facing output uses `println!`/`eprintln!`
  - raw agent output is persisted to `.metastack/agents/sessions/scan.log` and install-scoped listen logs.
- Generated content:
  - backlog files come from `tmp/_TEMPLATE/`
  - workflow prompts compose repo scope, overlays, and codebase docs before agent execution.
- Background work:
  - `tokio` is used at the binary boundary, but many subsystems still use threads and blocking process/file APIs.
- Storage model:
  - Markdown, JSON, and local log files are the primary persistence layer; no cache or database service exists.

---
*Architecture analysis: 2026-03-16*

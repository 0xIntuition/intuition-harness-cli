# Codebase Structure
**Analysis Date:** 2026-03-16

## Directory Layout
```text
.
├── .github/
│   └── workflows/
├── .metastack/
│   ├── agents/
│   ├── backlog/
│   ├── codebase/
│   ├── cron/
│   └── workflows/
├── .planning/
├── docs/
├── prompts/
├── scripts/
├── src/
│   ├── agents/
│   ├── cron/
│   ├── linear/
│   ├── listen/
│   └── tui/
├── tests/
│   └── support/
├── tmp/
│   └── _TEMPLATE/
└── workflows/
    └── builtin/
```

## Directory Purposes
- `src/`: main Rust application code for all command families.
- `src/linear/`: Linear domain types, service rules, rendering, CRUD forms, refine flow, and GraphQL transport.
- `src/listen/`: unattended listener orchestration, worker launching, browser dashboard, workpad handling, workspace provisioning, and persisted state.
- `src/agents/`: local-agent prompt/command execution and kickoff brief generation.
- `src/cron/`: detached cron runtime helpers.
- `src/tui/`: shared terminal input field components.
- `tests/`: integration-style CLI tests grouped by command family (`tests/listen.rs`, `tests/scan.rs`, `tests/linear.rs`, and others).
- `tests/support/`: shared test helpers for temp repos, stub agents, env cleanup, mock servers, and git setup.
- `tmp/_TEMPLATE/`: canonical backlog template files embedded by `src/backlog.rs` and copied during setup.
- `workflows/builtin/`: built-in Markdown workflow playbooks bundled into the binary by `src/workflows.rs`.
- `scripts/`: shell entry points for installation and release packaging.
- `docs/`: maintainer-facing design and release runbooks.
- `.metastack/`: active repo-local runtime workspace and generated context used by the CLI itself.
- `.planning/`: older planning/backlog artifacts that are still present in the repo but are not the primary runtime workspace path anymore.

## Key File Locations
- Binary entry:
  - `src/main.rs`
- Central dispatch:
  - `src/lib.rs`
  - `src/cli.rs`
- Repo bootstrap and scaffolding:
  - `src/setup.rs`
  - `src/scaffold.rs`
- Codebase scan/context:
  - `src/scan.rs`
  - `src/context.rs`
  - `src/workflow_contract.rs`
- Backlog/template logic:
  - `src/backlog.rs`
  - `src/plan.rs`
  - `src/technical.rs`
  - `src/sync_command.rs`
- Linear integration:
  - `src/linear/command.rs`
  - `src/linear/service.rs`
  - `src/linear/transport.rs`
- Listener and workspaces:
  - `src/listen/mod.rs`
  - `src/listen/worker.rs`
  - `src/listen/workspace.rs`
  - `src/listen/store.rs`
- Cron:
  - `src/cron.rs`
  - `src/cron/runtime.rs`
- Release/install helpers:
  - `scripts/install-meta.sh`
  - `scripts/release-artifacts.sh`

## Naming Conventions
- Command-family source files use domain names: `src/scan.rs`, `src/setup.rs`, `src/sync_command.rs`.
- Nested subsystems use directories with `mod.rs` or focused siblings: `src/linear/`, `src/listen/`.
- Test files mirror command or subsystem names: `tests/scan.rs`, `tests/workflows.rs`, `tests/cron.rs`.
- Generated workspace files use uppercase context doc names under `.metastack/codebase/`: `SCAN.md`, `ARCHITECTURE.md`, `TESTING.md`.

## Where to Add New Code
- New top-level CLI command:
  - add arg structs/enums in `src/cli.rs`
  - add dispatch in `src/lib.rs`
  - create a focused module under `src/` or `src/<family>/`
  - add a matching integration test file or extend the relevant `tests/*.rs`
- New Linear capability:
  - add or extend types in `src/linear/types.rs` if needed
  - add service rules in `src/linear/service.rs`
  - add GraphQL transport/query code in `src/linear/transport.rs`
  - expose it through `src/linear/command.rs`
  - cover it with `httpmock`-based tests in `tests/linear.rs`
- New `.metastack` generated file or scaffolded asset:
  - extend `PlanningPaths` in `src/fs.rs`
  - seed files in `src/scaffold.rs` or `src/backlog.rs`
  - update scan/setup tests that assert workspace structure
- New workflow playbook:
  - add a Markdown file under `workflows/builtin/` for built-ins
  - or add a repo-local file under `.metastack/workflows/` for runtime-only use
  - document expected parameters and validation steps in YAML front matter
- New backlog template file:
  - add it under `tmp/_TEMPLATE/`
  - extend `CANONICAL_TEMPLATE_FILES` in `src/backlog.rs`
  - update setup/scaffold tests that compare seeded template output

## Special Directories
- `.metastack/`:
  - runtime workspace used by the CLI for repo defaults, generated context, workflows, backlog sync artifacts, and cron jobs.
- `.planning/`:
  - legacy/planning content committed in the repo; useful as reference, but active runtime scaffolding targets `.metastack/`.
- `docs/`:
  - contributor and maintainer documentation such as `docs/agent-daemon.md` and `docs/manual-releases.md`.
- `scripts/`:
  - portable shell utilities for installation and release asset creation.
- `tmp/`:
  - source-of-truth backlog template content embedded into the binary and seeded into `.metastack/backlog/_TEMPLATE/`.
- `target/`:
  - Cargo build output; not part of source design.

---
*Structure analysis: 2026-03-16*

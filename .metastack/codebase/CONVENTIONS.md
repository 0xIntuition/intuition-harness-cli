# Coding Conventions
**Analysis Date:** 2026-03-16

## Naming Patterns
- Files and modules use `snake_case`: `src/sync_command.rs`, `src/listen/workspace.rs`.
- Public structs/enums use `PascalCase`: `PlanningMeta`, `LinearService`, `TicketWorkspace`.
- Functions and local bindings use `snake_case`: `run_scan`, `load_required_planning_meta`, `default_project_id`.
- Constants use `SCREAMING_SNAKE_CASE`: `DEFAULT_LINEAR_API_URL`, `BACKLOG_STATE`, `ISSUES_PAGE_SIZE`.
- Command entry points follow `run_*` naming and live near their subcommand family.

```rust
pub async fn run_plan(args: &PlanArgs) -> Result<PlanReport> { /* ... */ }
const DEFAULT_LISTEN_POLL_INTERVAL_SECONDS: u64 = 7;
```

## Code Style
- Formatting is delegated to `cargo fmt`; CI enforces it via `make quality` and `.github/workflows/quality.yml`.
- Linting is `cargo clippy --all-targets --all-features -- -D warnings`.
- The codebase prefers straightforward control flow, explicit `match` arms, and `anyhow::Result` over custom error enums.
- Multiline user-facing text often uses raw string literals for prompts, templates, and shell usage text.

```rust
#[derive(Debug, Parser)]
#[command(name = "meta", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}
```

## Import Organization
- Imports are usually grouped as:
  - `std::*`
  - third-party crates
  - `crate::*`
- Re-export modules are collected in `mod.rs` or `src/lib.rs`.
- There are no path aliases beyond normal Rust module paths.

```rust
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::fs::{PlanningPaths, canonicalize_existing_dir};
```

## Error Handling
- Command and service functions usually return `anyhow::Result<T>`.
- Validation errors use `anyhow!` or `bail!`.
- I/O and subprocess boundaries add `Context` with the file path or command that failed.
- Errors are surfaced once at the top in `src/main.rs`, which prints `error: {error:#}`.

```rust
let contents = fs::read_to_string(&path)
    .with_context(|| format!("failed to read `{}`", path.display()))?;

if !output.status.success() {
    bail!("git {} failed: {}", args.join(" "), stderr.trim());
}
```

## Logging
- There is no `tracing` or logger crate in current use.
- User-visible status uses `println!` and `eprintln!`.
- Long-running/raw command output is persisted to files such as `.metastack/agents/sessions/scan.log` or the listener log directory from `src/listen/store.rs`.

```rust
eprintln!("hint: `{legacy_command}` is a compatibility alias; prefer `{preferred_command}`.");
println!("{}", report.render());
```

## Comments
- Comments are sparse and usually reserved for intent, compatibility, or safety boundaries.
- Inline comments explain why, not what, for non-obvious behavior.

```rust
// Keep the source repo pointed at the latest upstream main as well.
let _ = run_git(root, &["fetch", "origin", "main"]);
```

## Function Design
- Public behavior is commonly exposed as small `run_*` orchestration functions that delegate to helpers.
- Argument-heavy commands prefer typed arg structs from `src/cli.rs` over long positional parameter lists.
- Rendering/reporting often uses lightweight structs with a `render()` method instead of formatting inline everywhere.

```rust
pub fn run_scan(args: &ScanArgs) -> Result<ScanReport> { /* ... */ }

impl ScanReport {
    pub fn render(&self) -> String { /* ... */ }
}
```

## Module Design
- Top-level subsystems map cleanly to files or subdirectories: `src/linear/`, `src/listen/`, `src/cron/`.
- `src/lib.rs` is the central dispatch layer and re-exports only what sibling modules need.
- `src/linear/mod.rs` is a typical module barrel for service, transport, render, and type exports.
- Rust-style named items are used throughout; there is no default-export analogue.

```rust
mod service;
mod transport;

pub use service::LinearService;
pub use transport::{LinearClient, ReqwestLinearClient};
```

## Template Literals & Formatting
- `format!` is the default for dynamic text.
- Raw string literals (`r#"... "#`) are used for GraphQL queries, Markdown templates, YAML front matter, and shell help text.
- Embedded files use `include_str!` for built-in workflows and backlog templates.

```rust
const PROJECTS_QUERY: &str = r#"
query Projects($first: Int!) { /* ... */ }
"#;

let status = format!("Created `{scan_display_path}`");
```

## Null/Undefined Handling
- `Option` and `Result` are preferred over sentinel values.
- Common patterns include `.as_deref()`, `.filter(...)`, `.unwrap_or_default()`, `.unwrap_or_else(...)`, and `.is_some_and(...)`.
- Optional config values are normalized early, then resolved into concrete defaults close to execution.

```rust
let instructions = args.instructions.as_deref().unwrap_or("");
let unattended = std::env::var("METASTACK_LISTEN_UNATTENDED")
    .ok()
    .is_some_and(|value| value == "1");
```

---
*Convention analysis: 2026-03-16*

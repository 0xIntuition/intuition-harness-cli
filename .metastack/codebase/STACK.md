# Technology Stack
**Analysis Date:** 2026-03-16

## Languages
- Primary: Rust (`edition = "2024"`) from `Cargo.toml`.
- Secondary:
  - Shell for install/release scripts in `scripts/install-meta.sh` and `scripts/release-artifacts.sh`
  - Markdown for built-in workflows, docs, and generated `.metastack/` artifacts
  - JSON/TOML/YAML for repo metadata, config, and cron front matter

## Runtime
- Runtime environment:
  - native CLI binary `meta` from `src/main.rs`
  - Tokio `1.50.0` for the async entry point and async Linear calls
- Package/build manager:
  - Cargo via `Cargo.toml`
- Config/runtime location model:
  - install-scoped config in TOML
  - repo-scoped `.metastack/` workspace in JSON/Markdown

## Frameworks
- `clap 4.6.0`: command parsing and help generation in `src/cli.rs`.
- `tokio 1.50.0`: async runtime for the CLI entry point and networked commands.
- `reqwest 0.13.2` with `rustls`: HTTP client for Linear GraphQL and file transfer in `src/linear/transport.rs`.
- `ratatui 0.30.0` and `crossterm 0.29.0`: terminal dashboards and interactive forms.
- `serde 1.0.228`, `serde_json 1.0.149`, `serde_yaml 0.9.34`, `toml 1.0.6`: config, state, and front matter serialization.

## Testing & Quality
- Test runner:
  - `cargo test`
- CLI/integration testing:
  - `assert_cmd 2.2.0`
  - `predicates 3.1.4`
  - `tempfile 3.27.0`
  - `httpmock 0.8.3`
- Quality commands:
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test`
  - `cargo test --test release_artifacts`
  - `make quality`
- CI:
  - GitHub Actions workflows in `.github/workflows/quality.yml` and `.github/workflows/release.yml`

## Key Dependencies
- CLI and UX:
  - `clap 4.6.0` for subcommands, args, help text, and env-backed flags
  - `crossterm 0.29.0` and `ratatui 0.30.0` for terminal UIs
- Async and scheduling:
  - `tokio 1.50.0` for async execution
  - `cron 0.15.0` for cron expression parsing
  - `chrono 0.4.42` and `time 0.3.44` for timestamps and formatting
- HTTP and external APIs:
  - `reqwest 0.13.2` for Linear GraphQL and file downloads/uploads
- Serialization and config:
  - `serde 1.0.228`
  - `serde_json 1.0.149`
  - `serde_yaml 0.9.34`
  - `toml 1.0.6`
- Filesystem and traversal:
  - `walkdir 2.5.0`
  - `ignore 0.4.25`
- Utility/error handling:
  - `anyhow 1.0.102`
  - `async-trait 0.1.89`
  - `libc 0.2.183` for process checks on supported platforms

## Configuration
- Root manifest:
  - `Cargo.toml`
- Build/test task runner:
  - `Makefile`
- Repo-local state and defaults:
  - `.metastack/meta.json`
  - `.metastack/codebase/*.md`
  - `.metastack/backlog/`
  - `.metastack/cron/`
- Install-scoped config:
  - resolved by `src/config.rs` from `$METASTACK_CONFIG` or XDG/HOME defaults
- CI and release:
  - `.github/workflows/quality.yml`
  - `.github/workflows/release.yml`

## Platform Requirements
- Development:
  - stable Rust toolchain with `cargo`, `rustfmt`, and `clippy`
  - local `git` for workspace cloning and branch operations
  - optional `codex` or `claude` executables on `PATH` for agent-backed commands
- Release/build:
  - `cross` for Linux musl cross-compiles in release automation
  - `tar`, `curl`, and `shasum` or `sha256sum` for packaging/install flows
- Production/runtime:
  - macOS or Linux target support is built into the release workflow
  - interactive dashboards require a TTY; unattended/browser dashboard features do not

---
*Stack analysis: 2026-03-16*

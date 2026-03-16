# External Integrations
**Analysis Date:** 2026-03-16

## APIs & External Services
- Linear GraphQL API:
  - Wired through `reqwest 0.13.2` in `src/linear/transport.rs`.
  - Used by issue/project listing, issue create/edit, workpad comment updates, attachment upload/delete, and file download.
  - Default endpoint is `https://api.linear.app/graphql` from `src/config.rs`.
- Local agent CLIs:
  - Built-in provider presets are `codex` and `claude` in `src/config.rs`.
  - Execution is handled by subprocess spawning in `src/agents/execution.rs`, `src/scan.rs`, `src/listen/worker.rs`, and `src/cron/runtime.rs`.
  - This is an external-tool integration, not an SDK integration.
- GitHub Releases:
  - `scripts/install-meta.sh` downloads release archives and `SHA256SUMS` from GitHub Releases via `curl`.
  - `.github/workflows/release.yml` publishes tagged release assets with `gh release`.

## Data Storage
- Repo-local filesystem storage:
  - `.metastack/meta.json` for repo defaults.
  - `.metastack/backlog/<ISSUE>/` for synced/generated Markdown artifacts.
  - `.metastack/codebase/*.md` for scan outputs.
  - `.metastack/cron/` and `.metastack/cron/.runtime/` for scheduled jobs and runtime state.
- Install-scoped filesystem storage:
  - `src/config.rs` resolves a data root next to the active config path.
  - `src/listen/store.rs` stores listener metadata, state, locks, and logs under `<config-parent>/data/listen/projects/<project-key>/`.
- No database, Redis, S3-compatible object store, or external queue service is present.

## Authentication & Identity
- Linear auth:
  - Primary credential is `LINEAR_API_KEY`.
  - Resolved from CLI flags, global config, named profiles, or env in `src/config.rs`.
- GitHub auth:
  - Release workflow uses `GH_TOKEN` from GitHub Actions in `.github/workflows/release.yml`.
- Local identity:
  - Listener project identity is derived from the canonical source `.metastack` root hash in `src/listen/store.rs`.
- No OAuth, SSO, session cookies, or user-login system exists inside the CLI itself.

## Email Service
- No email provider integration is present.

## Monitoring & Observability
- No SaaS observability integration such as Sentry, Datadog, Honeycomb, or OpenTelemetry is configured.
- Operational visibility is local:
  - stdout/stderr for interactive commands
  - `.metastack/agents/sessions/*.log` for scan/agent output
  - install-scoped listen logs from `src/listen/store.rs`
  - `.metastack/cron/.runtime/*.log` for cron execution

## CI/CD & Deployment
- GitHub Actions:
  - `.github/workflows/quality.yml` runs `make quality`.
  - `.github/workflows/release.yml` validates tags, builds release archives, and publishes GitHub Releases.
- Rust toolchain:
  - CI installs stable Rust with `rustfmt` and `clippy` via `dtolnay/rust-toolchain@stable`.
- Distribution:
  - release archives are produced by `scripts/release-artifacts.sh`.
  - local installation is handled by `scripts/install-meta.sh`.

## Webhooks & Callbacks
- No inbound webhook server is implemented.
- Listener behavior is polling-based against Linear in `src/listen/mod.rs`.
- The only local HTTP server is the loopback dashboard in `src/listen/web.rs`; it serves status pages and `/health`, not external callbacks.

## Feature Flags & Configuration
- Install-scoped config:
  - TOML file from `$METASTACK_CONFIG`, `$XDG_CONFIG_HOME/metastack/config.toml`, or `~/.config/metastack/config.toml` in `src/config.rs`.
- Repo-scoped config:
  - `.metastack/meta.json` parsed by `PlanningMeta` in `src/config.rs`.
- Runtime switches:
  - agent choice/model/reasoning
  - listen assignment scope and refresh policy
  - required listen label and poll interval
  - Linear profile/team/project defaults
  - workflow/provider overrides from `src/workflows.rs`

## Platform-Specific Configurations
- CLI/TUI:
  - interactive flows use `crossterm 0.29.0` and `ratatui 0.30.0` in files such as `src/plan.rs`, `src/technical.rs`, and `src/cron_dashboard.rs`.
  - non-TTY mode falls back to text-only execution.
- Local browser dashboard:
  - `src/listen/web.rs` starts a loopback server on `127.0.0.1` and renders HTML status pages.
- OS support:
  - release assets target macOS and Linux in `.github/workflows/release.yml`.
  - scripts assume common Unix tooling (`sh`, `curl`, `tar`, `uname`).

## Environment Setup
```bash
# Required for live Linear commands unless stored in config.toml
export LINEAR_API_KEY=lin_api_replace_me

# Optional overrides
export METASTACK_CONFIG="$HOME/.config/metastack/config.toml"
export LINEAR_API_URL="https://api.linear.app/graphql"
export LINEAR_TEAM="MET"
```

## Security Considerations
- The repository currently tracks a `.env` file with a Linear token; rotate and remove it before treating the repo as safe to share.
- Agent and cron integrations execute local commands with repo-derived context, so repository trust matters.
- Listener workers propagate issue/workspace metadata through environment variables in `src/listen/worker.rs`; avoid logging those values unnecessarily in external wrappers.
- The local dashboard is loopback-only by default, but it still exposes issue/session details to anything that can reach the bound port.

---
*Integration audit: 2026-03-16*

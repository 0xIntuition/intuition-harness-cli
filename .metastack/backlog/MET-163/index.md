# Add advanced agent-routing config, resolver, config surfaces, and route-aware execution across agent-backed commands

Introduce a new install-scoped agent routing model in `src/config.rs` that can resolve agents for specific CLI commands and command families instead of relying on a single `default_agent`, then wire all current agent-backed execution paths to use explicit route keys and the shared resolver.

This work should define stable route keys for all supported agent-backed workflows, implement fallback resolution as `command override -> command family default -> global default`, validate config edits, expose the routing data in machine-readable config output, provide both non-interactive editing and a dedicated advanced dashboard in the `meta runtime config` family, and update each execution path to declare and use its route consistently while still honoring explicit CLI overrides like `--agent`, `--model`, and `--reasoning`. The primary config flow should remain simple, with advanced routing kept as an explicit opt-in surface for users who want per-command control.

## Acceptance Criteria

- `AppConfig` supports advanced agent routing entries for command-specific and family-level defaults in install-scoped config
- A shared resolver can answer the effective provider/model/reasoning for a named CLI route using `command override -> command family default -> global default` fallback
- The resolver covers all current agent-backed flows, including backlog planning/splitting, scan/context refresh, issue refine, workflows, cron prompt jobs, and listen worker execution
- Config validation rejects unknown agent names, invalid model/provider combinations, and malformed route keys with clear errors
- `meta runtime config --json` includes the advanced agent routing map in a stable machine-readable shape
- Non-interactive config flags or subcommands can set and clear global, family, and command-specific agent defaults without requiring the dashboard
- Direct config edits validate route keys and provider/model compatibility before writing the TOML file
- The non-interactive surface can update routes for agent-backed commands such as `backlog plan`, `backlog split`, `agents listen`, `context scan`, `linear issues refine`, workflow runs, and cron prompt jobs
- `meta runtime config` exposes a dedicated advanced dashboard flow for agent routing rather than mixing every routing option into the primary simple config steps
- The dashboard lists agent-backed command families and individual command routes, showing each route's effective agent and inheritance source
- Users can set, change, and clear family-level and command-level overrides from the dashboard
- Each agent-backed command declares a stable route key and resolves its effective provider/model/reasoning through the shared routing resolver
- Explicit CLI overrides still take precedence over config-based routing for the active run
- `meta backlog plan` and `meta backlog split` can use a different configured default agent than `meta agents listen`
- `meta context scan`/scan refresh, `meta linear issues refine`, workflow runs, and cron prompt jobs all honor route-specific defaults
- Automated tests cover route resolution precedence, TOML round-tripping, JSON output, dashboard rendering, and at least one end-to-end save flow for route overrides
- Regression tests prove at least two different command routes can resolve to different agents in the same config file
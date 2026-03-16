# Decisions: Technical: Technical: Add multi-provider registry, approval profiles, and budget governance for fleet operations

Last updated: 2026-03-16

Record meaningful scope, design, and rollout decisions here.

## Decision Log

### D-001: Expose governance through dedicated command families

- Date: 2026-03-16
- Status: proposed
- Context: Operators need to inspect provider choice, approval posture, and budget status directly from the CLI. The current command tree has `meta runtime config` for install-scoped settings, but it does not expose effective runtime governance.
- Decision: Prefer explicit `meta providers`, `meta policy`, and `meta budgets` command families, with compatibility aliases only if they materially reduce migration risk.
- Consequences: `src/cli.rs` and `src/lib.rs` will grow new subcommand branches, but operator workflows remain discoverable and reviewer-oriented.

### D-002: Preserve single-agent config as an implicit default registry entry

- Date: 2026-03-16
- Status: proposed
- Context: Current repos rely on install-scoped `agents.default_agent`, `agents.default_model`, and repo-local `.metastack/meta.json` agent fields.
- Decision: Treat the existing single-agent configuration as a compatibility path that resolves to an implicit provider profile when no explicit provider registry is configured.
- Consequences: Existing repos keep working without migration, and the new registry code must normalize both legacy and new config shapes into one runtime resolution model.

### D-003: Persist usage and cost evidence as append-only JSONL under `.metastack/agents/sessions/`

- Date: 2026-03-16
- Status: proposed
- Context: The repository already stores scan logs and listen session state under `.metastack/agents/sessions/`, but cost and routing evidence is not durable or queryable.
- Decision: Add a machine-readable append-only event ledger in the sessions area instead of deriving budget status from ad hoc logs.
- Consequences: Status commands and dashboards can share one source of truth, and filesystem validation becomes straightforward.

### D-004: Approval profiles express provider-independent intent

- Date: 2026-03-16
- Status: proposed
- Context: Current approval behavior is embedded in provider-specific invocation details, such as Codex always receiving `--ask-for-approval never` in unattended workspace runs.
- Decision: Define approval profiles in neutral terms such as unattended, review-required, or interactive, then map them to provider-specific flags in the launch layer.
- Consequences: Operators can reason about policy without reading provider invocation code, but provider adapters must report unsupported approval modes clearly.

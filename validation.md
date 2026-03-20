# ENG-10234 Validation

## Summary

This ticket makes the visible CLI command name and repo-local operational state root configurable while preserving `.metastack/meta.json` as the canonical repo metadata location and keeping `meta` as the compatibility entry point.

## Completed Validation

- 2026-03-19: `cargo test`
- 2026-03-19: `cargo clippy --all-targets --all-features -- -D warnings`
- 2026-03-19: `cargo run -- runtime config --json`
  - Verified install-scoped branding/layout fields appear under `app.branding.command_name`, `app.branding.repo_state_root`, and `app.branding.backlog_root` without changing `METASTACK_CONFIG`.
- 2026-03-19: `cargo run -- runtime setup --root <tmp> --command-name intuition --repo-state-root .intuition --backlog-root .intuition/backlog`
  - Verified repo-scoped branding/layout fields persist in `<tmp>/.metastack/meta.json`.
- 2026-03-19: `cargo run -- runtime setup --root <tmp> --command-name intuition --repo-state-root .intuition --backlog-root .intuition/backlog --migrate-layout`
  - Verified rerunnable migration preserves canonical metadata at `<tmp>/.metastack/meta.json` while moving operational state under `<tmp>/.intuition/`.
- 2026-03-19: branded executable alias proof
  - Verified copying `target/debug/meta` to `/tmp/intuition-meta-alias` preserves version output and renders branded help text from the invoked binary name.
- 2026-03-19: `cargo test --test install_release --test release_artifacts -- --nocapture`
  - Verified `scripts/install-meta.sh --alias intuition` installs both `meta` and the branded alias with equivalent behavior.
  - Verified `scripts/release-artifacts.sh` packages a branded binary when `META_RELEASE_BINARY_NAME=intuition`.
- 2026-03-19: `cargo test -q --test scan`
- 2026-03-19: `cargo test -q --test sync`
- 2026-03-19: `cargo test -q listen::tests::workspace_snapshot_ignores_generated_agent_brief`
- 2026-03-19: direct branded-root scan proof
  - Verified `cargo run -- context scan --root <tmp-repo-with-.intuition-root>` writes codebase docs under `.intuition/codebase/` and the scan log under `.intuition/agents/sessions/scan.log`.
- 2026-03-20: `cargo test -q context_doctor_uses_branded_repo_state_root_for_codebase_inputs --test context`
- 2026-03-20: `cargo test -q sync_status_reads_entries_from_branded_backlog_root --test sync`
- 2026-03-20: `cargo test -q technical_command_writes_child_backlog_into_branded_root --test technical`
  - Verified `context doctor`, `backlog sync status`, and `backlog tech` respect branded repo-local state and backlog roots.
- 2026-03-20: `cargo test -q listen_uses_the_same_project_identity_for_repo_and_worktree_roots --test listen`
- 2026-03-20: `cargo test -q listen_uses_the_same_project_identity_for_branded_repo_and_worktree_roots --test listen`
- 2026-03-20: `cargo clippy --all-targets --all-features -- -D warnings`
  - Verified branded repos and worktrees share the same persisted listen project identity and `listen sessions inspect` reports the effective repo-local state root.

## Coverage Mapping

- Install-scoped branding/layout config: covered.
- Repo-scoped branding/layout persistence: covered.
- Branded repo-local context output path: covered.
- Branded backlog generation and sync path: covered.
- Migration from `.metastack/` to branded operational root: covered.
- Branded executable alias packaging/install path: covered.
- Compatibility of canonical `.metastack/meta.json`: covered.

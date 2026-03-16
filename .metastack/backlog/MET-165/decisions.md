# Decisions: Technical: Implement the end-to-end `meta merge` command, dashboard, agent merge-run orchestration, and aggregate PR publication

Last updated: 2026-03-16

Record meaningful scope, design, and rollout decisions here.

## Decision Log

### D-001: Add `meta merge` as a top-level, additive command family

- Date: 2026-03-16
- Status: proposed
- Context: current user-facing commands are grouped by domain in `src/cli.rs` and dispatched from `src/lib.rs`, with deterministic `--json` or `--render-once` variants for automation and tests.
- Decision: implement `meta merge` as a first-class command family with help text, TTY dashboard behavior, and deterministic non-interactive proof paths instead of hiding the feature behind `meta agents` or `meta dashboard` aliases.
- Consequences: the public CLI surface stays coherent, but the command must define clear behavior for TTY, non-TTY, and test-driven scripted interaction modes.

### D-002: Use the `gh` CLI as the GitHub transport boundary

- Date: 2026-03-16
- Status: proposed
- Context: the parent issue explicitly requires `gh`, and the repository currently has no GitHub SDK layer.
- Decision: implement a focused adapter that shells out to `gh` for repo resolution, open-PR discovery, PR metadata inspection, and aggregate PR create/update behavior.
- Consequences: contributor environments need `gh` installed and authenticated for live runs; tests must stub `gh` on `PATH`; error messages must call out missing auth or repo-resolution failures explicitly.

### D-003: Keep merge execution out of the source checkout and persist local audit artifacts

- Date: 2026-03-16
- Status: proposed
- Context: listener work already uses isolated workspace clones, and the parent issue requires deterministic local artifacts under `.metastack/merge-runs/<RUN_ID>/`.
- Decision: create or reuse a safe workspace outside the source checkout for every merge batch and record stable run files such as `repo.json`, `selection.json`, `merge-plan.md`, `status.json`, `validation.json`, and `aggregate-pr.md` under `.metastack/merge-runs/<RUN_ID>/`.
- Consequences: shared git/workspace helpers are likely worth extracting; the run manifest becomes part of the public local contract and must be tested directly.

### D-004: Keep agent orchestration one-shot and subprocess-based

- Date: 2026-03-16
- Status: proposed
- Context: the repository already launches configured local agents through subprocesses and injects repo context into prompts.
- Decision: invoke the configured local agent once for merge planning and again only when conflict resolution or integration repair is required, without turning `meta merge` into a background listener session.
- Consequences: prompt builders must include enough repo and PR context to support explicit merge ordering and blocker reporting; failures must stop with concrete artifact-backed diagnostics instead of silent partial success.

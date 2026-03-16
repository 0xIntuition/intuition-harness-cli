# Testing Patterns
**Analysis Date:** 2026-03-16

## Test Framework
- Primary runner: `cargo test`.
- CLI assertions: `assert_cmd 2.2.0` plus `predicates 3.1.4`.
- HTTP mocking: `httpmock 0.8.3` for Linear GraphQL and related flows.
- Temporary filesystem fixtures: `tempfile 3.27.0`.
- Quality gate commands:
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test`
  - `cargo test --test release_artifacts`
  - `make quality`

```rust
cli()
    .args(["scan"])
    .assert()
    .success()
    .stdout(predicate::str::contains("Codebase scan completed"));
```

## Test File Organization
- Tests live in top-level integration files under `tests/`.
- File names mirror command families or subsystems:
  - `tests/commands.rs`
  - `tests/config.rs`
  - `tests/linear.rs`
  - `tests/listen.rs`
  - `tests/scan.rs`
  - `tests/workflows.rs`
- Shared helpers are pulled in with `include!("support/common.rs");`.

```rust
#![allow(dead_code, unused_imports)]

include!("support/common.rs");
```

## Test Structure
- Most suites are scenario-oriented integration tests that build temp repos/configs, run the `meta` binary, and assert stdout/stderr plus filesystem side effects.
- Several runtime modules also keep local unit tests inline under `#[cfg(test)]`.
- Dashboard/TUI rendering is tested with snapshot-like string assertions or `ratatui::backend::TestBackend`.

```rust
#[test]
fn setup_json_scaffolds_repo_defaults() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    fs::create_dir_all(&repo_root)?;
    cli().args(["setup", "--root", repo_root.to_string_lossy().as_ref(), "--json"])
        .assert()
        .success();
    Ok(())
}
```

## Mocking
- Linear API interactions are mocked with `httpmock::MockServer`.
- Agent execution is often stubbed with temporary shell scripts that record env vars or prompt payloads.
- Git-heavy flows use temp repositories initialized by helpers in `tests/support/common.rs`.
- The codebase prefers mocking external boundaries, not internal helper functions.

```rust
let server = MockServer::start();
let api_url = server.url("/graphql");

let issues_mock = server.mock(|when, then| {
    when.method(POST).path("/graphql").body_includes("query Issues");
    then.status(200).json_body(json!({ "data": { "issues": { "nodes": [] }}}));
});
```

## Fixtures and Factories
- `tests/support/common.rs` is the main fixture toolkit.
- Common helpers include:
  - `write_minimal_planning_context`
  - `init_repo_with_origin`
  - `create_worktree_checkout`
  - `create_workspace_clone_checkout`
  - `listen_state_path`
- Test data is built inline with JSON literals, temp files, and small helper functions rather than a heavy factory framework.

```rust
write_minimal_planning_context(
    &repo_root,
    r#"{"linear":{"team":"MET"},"listen":{"poll_interval_seconds":42}}"#,
)?;
```

## Coverage
- No coverage tool or threshold is configured in the repo today.
- The enforced bar is green formatting, linting, and tests through `make quality`.
- `README.md` and CI both point contributors toward the Cargo/Make quality commands rather than a line-coverage metric.

## Test Types
- Unit tests: present inside modules such as `src/agents/mod.rs`, `src/workflow_contract.rs`, and dashboard modules.
- Integration tests: dominant test style under `tests/`.
- Snapshot-like tests: present for terminal/browser dashboard rendering.
- Script/release tests: present for installer and artifact flows (`tests/install_release.rs`, `tests/release_artifacts.rs`, `tests/release_workflow.rs`).
- Missing today:
  - browser automation tests
  - property/fuzz tests
  - real network end-to-end tests against GitHub/Linear

## Common Patterns
- Tests remove ambient env vars up front so developer machines do not leak state into assertions.
- Non-interactive CLI paths are preferred in tests.
- Predicate chaining is used heavily for stdout/stderr assertions.
- Temp shell scripts are used to prove env propagation and prompt rendering.

```rust
for key in TEST_ENV_REMOVALS {
    command.env_remove(key);
}

meta()
    .env("METASTACK_CONFIG", &config_path)
    .arg("listen")
    .assert()
    .failure();
```

## Test Execution Rules
- Keep tests deterministic:
  - isolate env vars
  - use temp dirs and temp repos
  - avoid relying on a real TTY unless explicitly testing TUI rendering
- Avoid `.only`/focused-test equivalents; none are used in current Rust tests.
- Prefer asserting exact side effects in `.metastack/`, temp files, or mocked HTTP requests, not just exit codes.
- Unix-specific behavior is guarded with `#[cfg(unix)]` where needed.

## Testing Data Patterns
- JSON-heavy responses are created inline with `serde_json::json!`.
- Repo fixtures are minimal: usually just enough files for `Cargo.toml`, `README.md`, and `.metastack/`.
- For agent tests, shell stubs write received values into temp output files for later assertions.

```rust
fs::write(
    &stub_path,
    r#"#!/bin/sh
printf '%s' "$METASTACK_AGENT_PROMPT" > "$TEST_OUTPUT_DIR/prompt.txt"
"#,
)?;
```

## Current Test Coverage
- Well-covered areas:
  - command routing and help output
  - setup/scaffold behavior
  - context/scan generation flow
  - Linear list/create/edit/refine command behavior
  - backlog plan/split/sync flows
  - listen state handling and many unattended edge cases
  - workflow playbook rendering and execution
  - cron init/runtime behavior
  - release/install scripts
- Thinner or missing areas:
  - real GitHub publication flow
  - transport timeout/retry behavior under slow networks
  - secret hygiene regression tests
  - multi-host scaling behavior for listener locks/state

---
*Testing analysis: 2026-03-16*

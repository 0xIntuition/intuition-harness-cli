# Validation - ENG-10510

## Required Checks

- `cargo test --test commands`
- `cargo test --test config`
- `cargo test --test technical`
- `cargo test --test backlog_split`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo run -- backlog --help`
- `cargo run -- backlog split --help`

## Deterministic Command Proofs

- Interactive `meta backlog split <ISSUE>`
  - Covered by `cargo test --test backlog_split`
  - `backlog_split_render_once_shows_review_flow_snapshot` proves the guided review flow renders the split session for `MET-35`
- Non-interactive `meta backlog split --no-interactive <ISSUE>`
  - Covered by `cargo test --test backlog_split`
  - `backlog_split_no_interactive_emits_structured_proposal_json` proves the command emits the `backlog.split` JSON envelope with child issues, parent rewrite, and dependency suggestions
- Apply path proof
  - Covered by `cargo test --test backlog_split`
  - `backlog_split_render_once_events_can_apply_split_end_to_end` proves reviewed splits create child issues, write one backlog packet per child, rewrite the parent, and create dependency links

## Results

- `cargo test --test commands`
  - passed
  - confirmed `meta backlog --help` lists `split` as a first-class subcommand
  - confirmed `meta backlog tech --help` no longer advertises `split`
  - confirmed `meta backlog split --help` exposes the inverse-planning surface

- `cargo test --test config`
  - passed
  - confirmed `backlog.tech` is accepted as a route key
  - confirmed legacy `backlog.split` command-route config still falls back for `backlog.tech`

- `cargo test --test technical`
  - passed
  - confirmed existing `meta backlog tech` child-derivation behavior still creates the technical child issue and local backlog files

- `cargo test --test backlog_split`
  - passed
  - confirmed non-interactive proposal JSON under the `backlog.split` envelope
  - confirmed render-once split review snapshot for an existing parent issue
  - confirmed end-to-end split apply creates child issues, writes `.metastack/backlog/<CHILD>/` packets, rewrites the parent, and creates dependency links

- `cargo clippy --all-targets --all-features -- -D warnings`
  - passed

- `cargo run -- backlog --help`
  - passed
  - showed `split` alongside `spec`, `plan`, `improve`, `tech`, and `sync`

- `cargo run -- backlog split --help`
  - passed
  - showed `intu backlog split [OPTIONS] <IDENTIFIER>` plus `--state`, `--priority`, `--label`, `--assignee`, and `--no-interactive`

## Full Quality Gate

- `make quality`
  - attempted
  - `cargo fmt --check` passed
  - `cargo clippy --all-targets --all-features -- -D warnings` passed
  - the serial full-suite phase surfaced existing failures in `tests/listen.rs` before completion:
    - `listen_worker_promotes_the_same_draft_pull_request_during_review_handoff`
    - `listen_worker_publishes_a_pull_request_after_push_without_a_local_remote_tracking_ref`
    - `listen_worker_publishes_the_initial_branch_pull_request_as_a_draft`
    - `listen_worker_reuses_stored_codex_resume_handle`
    - `listen_worker_reuses_stored_provider_native_resume_handle`
  - ENG-10510 targeted validation and command proofs remained green

## Acceptance Criteria Mapping

| Criterion | Evidence |
| --- | --- |
| `meta backlog --help` lists `split` and `meta backlog tech` no longer advertises `split` | `cargo test --test commands`; `cargo run -- backlog --help`; `cargo run -- backlog split --help` |
| `meta backlog split --no-interactive <ISSUE>` emits structured JSON proposals under `backlog.split` | `cargo test --test backlog_split` (`backlog_split_no_interactive_emits_structured_proposal_json`) |
| Interactive split runs stay inside one ratatui flow | `cargo test --test backlog_split` (`backlog_split_render_once_shows_review_flow_snapshot`) |
| Approved split runs create child issues, backlog packets, parent rewrite, and dependency links | `cargo test --test backlog_split` (`backlog_split_render_once_events_can_apply_split_end_to_end`) |
| Existing `meta backlog tech` behavior and routing remain green | `cargo test --test technical`; `cargo test --test config` |

# Validation Plan

## Command Proofs

- Run the changed CLI flow against a deterministic local or mocked setup
- Verify the original Linear issue description for `ENG-10168` remains unchanged
- Update the existing `## Codex Workpad` comment with validation notes instead of running `meta sync push`

## Current Evidence

- [x] `cargo test linear::ticket_context::tests -- --nocapture`
- [x] `cargo test linear::transport::tests -- --nocapture`
- [x] `cargo test technical::tests::technical_prompt_includes_selected_criteria_and_repo_snapshot -- --nocapture`
- [x] `cargo test sync_pull_restores_issue_description_and_managed_attachment_files -- --nocapture`
- [x] `cargo test technical_command_creates_a_child_issue_and_local_backlog_files -- --nocapture`

## Notes

- `meta listen` must not overwrite the primary Linear issue description.

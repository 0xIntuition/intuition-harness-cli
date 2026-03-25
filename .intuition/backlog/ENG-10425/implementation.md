# Implementation Plan

## Workstreams

1. Define the shared copy/export contract under `src/tui`, including payload shape, clipboard outcome model, help/status utilities, and fallback overlay behavior.
2. Migrate planning and backlog workflows so request summaries, question prompts, answers, reviews, and markdown previews use the shared contract.
3. Extend `InputFieldState` and roll the editor-side contract through multiline forms plus single-line setup/config/onboarding/Linear editors.
4. Adopt the same full-pane copy interaction across dashboard preview/detail/summary panes, then finish docs and the repo-wide audit matrix.

## Touchpoints

- Shared `src/tui` contract:
  - `../../../src/tui/mod.rs`
  - `../../../src/tui/keybindings.rs`
  - `../../../src/tui/fields.rs`
  - `../../../src/tui/markdown.rs`
  - `../../../src/tui/scroll.rs`
  - `../../../src/tui/copy.rs`
- Planning and backlog workflows:
  - `../../../src/plan.rs`
  - `../../../src/backlog_spec.rs`
  - `../../../src/technical.rs`
  - `../../../src/backlog_improve.rs`
- Shared fields and editors:
  - `../../../src/cron_dashboard.rs`
  - `../../../src/workflows.rs`
  - `../../../src/setup.rs`
  - `../../../src/config_command.rs`
  - `../../../src/onboarding.rs`
  - `../../../src/linear/create.rs`
  - `../../../src/linear/edit.rs`
- Operational dashboards and focused panes:
  - `../../../src/sync_dashboard.rs`
  - `../../../src/merge_dashboard.rs`
  - `../../../src/review/mod.rs`
  - `../../../src/improve/dashboard.rs`
  - `../../../src/linear/dashboard.rs`
  - `../../../src/listen/dashboard.rs`
  - `../../../src/workspace_dashboard.rs`
- Docs and backlog packet:
  - `../../../README.md`
  - `../../../docs/workflows-run-tui.md`
  - `../../../docs/agent-daemon.md`
  - this backlog packet under `.intuition/backlog/ENG-10425/`

## Notes on Interfaces

- CLI entrypoints: no new top-level commands are required; the change is shared behavior inside existing TUIs.
- Linear mutations/queries: none are required for the feature itself beyond the existing ticket/workpad flow.
- Local packet updates: record the final surface audit, validation proof, and rollout status here as code lands.

## Expected Changes

- Introduce one shared copy/export abstraction instead of per-surface clipboard helpers.
- Reuse shared markdown and scroll helpers so copied payloads stay aligned with displayed content.
- Add field-side select-all/copy semantics to `InputFieldState` without changing existing paste behavior.
- Update help and status copy across adopted surfaces to one shared contract.
- Add focused unit, integration, snapshot, and render-once coverage strong enough to enforce clipboard and fallback behavior.

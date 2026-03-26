# Specification: Technical: Implement a repo-wide copyable TUI contract across all interactive surfaces

Version: 0.2  
Last updated: 2026-03-25

Parent index: [`./index.md`](./index.md)

## 1. Executive Summary

Deliver one shared TUI copy/export contract for `metastack-cli` so interactive panes and editors expose the same copy interaction, the same help and status language, and the same deterministic fallback behavior when direct clipboard writes are unavailable.

## 2. Problem Statement

- Problem: the repository has many interactive TUI surfaces but no shared copyability contract.
- Why now: the parent issue explicitly scopes this as a repo-wide contract and names the rollout order.
- Non-goals:
  - redesigning every TUI layout or focus model
  - replacing [`../../../src/tui/markdown.rs`](../../../src/tui/markdown.rs)
  - changing prompt-image paste behavior beyond required editor-copy integration
  - adding required GUI clipboard dependencies for normal TUI operation

## 3. Functional Requirements

1. A shared `src/tui` API must represent copyable content for both read-only panes and editable fields.
2. Every adopted surface must expose a required plain-text payload.
3. Markdown-backed surfaces must preserve source markdown in the shared payload when it exists.
4. The contract must return deterministic clipboard success, clipboard failure, and clipboard-unavailable fallback outcomes.
5. Read-only panes must copy full logical content, not viewport-only text.
6. `InputFieldState` must own the shared field selection/copy model for single-line and multiline editors.
7. Shared keybinding/help/status text must be defined once and reused across adopted surfaces.
8. Planning/backlog panes, field-backed forms, and focused dashboard detail panes must all converge on the same contract.

## 4. Non-Functional Requirements

- Performance: copy/export actions must operate on already-available pane or field data.
- Reliability: identical content plus identical environment capability must yield identical outcomes and status text.
- Security: fallback/export content must stay in-process and terminal-safe.
- Observability: copy actions must expose deterministic status messaging that snapshot or focused tests can assert.

## 5. Contracts and Interfaces

### 5.1 Inputs

- required plain-text content derived from the underlying pane or field state
- optional markdown source for markdown-authored panes
- a surface label used in status and fallback messaging
- environment capability for direct clipboard writes
- field-selection metadata when the source is an editable `InputFieldState`

Validation rules:

- plain text is mandatory for every adopted surface
- markdown is optional and only present when source markdown exists
- fallback output must remain textual and terminal-safe
- read-only panes export full logical content
- field selections stay within the current text buffer and never include prompt-attachment binary payloads

### 5.2 Outputs

- a normalized copy payload with plain text and optional markdown
- a deterministic outcome enum for clipboard success, clipboard failure, and fallback-ready export
- shared help and status text for copy success, fallback, and failure
- optional fallback export content that renders fully in-terminal

Error shape:

- direct clipboard invocation errors surface as structured failure outcomes
- unsupported environments resolve to the fallback path, not a silent no-op
- invalid field-selection bounds stay local to the TUI and do not corrupt editor state

### 5.3 Compatibility

- Markdown pane rendering continues to flow through [`../../../src/tui/markdown.rs`](../../../src/tui/markdown.rs).
- Wrapped scroll behavior continues to flow through [`../../../src/tui/scroll.rs`](../../../src/tui/scroll.rs).
- Existing editor behavior in [`../../../src/tui/fields.rs`](../../../src/tui/fields.rs) stays compatible with prompt-image paste and multiline entry.
- Existing focus and navigation patterns may stay command-specific as long as the copy contract itself is shared.

## 6. Architecture and Data Flow

1. A focused pane or field requests a shared copy/export action from a new `src/tui` copy layer.
2. The source surface provides required plain text and optional markdown source.
3. The shared layer resolves whether direct clipboard write is available for the current environment.
4. If available, the shared layer attempts the direct clipboard write and returns a stable success or failure outcome.
5. If unavailable, the shared layer returns a stable fallback-ready outcome and the caller renders the terminal-safe export overlay.
6. Shared keybinding/help/status helpers keep copy language identical across adopted surfaces.

## 7. Acceptance Criteria

- [ ] A shared `src/tui` copy/export API exists for read-only panes and editable fields.
- [ ] Clipboard success, clipboard failure, and fallback behavior are deterministic and covered by targeted tests.
- [ ] Planning/backlog panes can copy request, question, review, and preview content without leaving the workflow.
- [ ] `InputFieldState` exposes the shared field selection/copy model without regressing paste, newline insertion, scrolling, or prompt attachments.
- [ ] Focused dashboard/detail panes expose the same full-pane copy interaction.
- [ ] README/docs plus a repo-wide audit matrix document adopted surfaces.

## 8. Test Plan

- Unit tests: payload normalization, capability detection, clipboard outcome classification, and field selection/copy behavior in `src/tui`.
- Integration tests: planning/backlog flows in `src/plan.rs`, `src/backlog_spec.rs`, `src/technical.rs`, and `src/backlog_improve.rs`; field consumers in setup/config/onboarding/cron/workflows/Linear forms; dashboard panes in sync, merge, review, improve, Linear, listen, and workspace surfaces.
- Contract tests: direct clipboard success, direct clipboard failure, and clipboard-unavailable fallback.
- Negative-path tests: invalid field selection bounds, missing markdown source on markdown-rendered panes, and copy support that must not regress scrolling or submission behavior.

## 9. Open Questions

1. Whether fallback/export should stay as one shared overlay everywhere or graduate into a richer reusable review/export state.
2. Whether markdown-backed panes need a second explicit raw-markdown action in v1.
3. Whether any non-markdown dashboards need curated export summaries instead of full-pane plain text.

## 10. Linked Workstreams

- Shared copy contract and deterministic outcomes: [`./tasks/workstream-template.md`](./tasks/workstream-template.md)

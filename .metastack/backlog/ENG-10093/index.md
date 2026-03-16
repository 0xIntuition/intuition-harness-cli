# Open dashboards immediately and load Linear data asynchronously across listen, backlog tech, and backlog sync

## Problem
`meta listen`, `meta backlog tech`, and `meta backlog sync` can spend noticeable time fetching Linear data before the TUI appears, which makes the commands feel hung even when work is progressing.

## Scope
Add a shared dashboard bootstrap/loading pattern so these commands enter the alternate-screen UI immediately, show a clear loading state, and perform initial Linear fetches in the background. Separate TUI redraw cadence from Linear polling so the interface refreshes once per second even when the Linear refresh interval is slower.

## Notes
Keep the issue scoped to commands in this repository that need this loading behavior, with shared infrastructure where possible and command-specific adapters only where necessary.

## Acceptance Criteria

- `meta listen`, `meta backlog tech`, and bare `meta backlog sync` open their TUI immediately before waiting on initial Linear/network work, and show a visible loading state until the first dataset is ready.
- Dashboard rendering refreshes once per second independently of Linear polling/loading cadence, so countdowns and loading indicators continue updating while background fetches are in progress.
- Loading, success, and failure states are covered by deterministic tests for each affected command path, including at least one proof that the TUI can render before the first Linear response arrives.
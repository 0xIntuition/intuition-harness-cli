# Fix `meta agents listen` attachment/artifact context bootstrap noise for Todo issue pickup

## Problem
`meta agents listen` can pick up a Todo issue, create the local backlog and workpad, then report an attachment/artifact context failure during startup even though the worker continues. The listener currently folds attachment-context download results into the pickup summary, so when zero files download it can surface a failure-style status even if local backlog context is already present and execution is not actually blocked.

## Scope
Trace the listener pickup/bootstrap path in `src/listen/mod.rs` and related listen modules to identify the exact scenarios that produce the current failure signal for Todo issues with local backlog state. Add deterministic regression coverage for those cases, then adjust listener bootstrap behavior so attachment/artifact context failures are only surfaced as blocking errors when they actually prevent required listener execution. If the local backlog is ready and the worker can proceed, the dashboard/session summary should stay compact and avoid implying startup failure. Also persist concise diagnostics in an existing listener artifact/log so operators can tell which attachment or file was missing or failed without adding per-file dashboard spam.

## Notes
Keep the change scoped to the listen command. Do not change backlog sync or broader attachment semantics unless required to safely reproduce and fix the listener behavior. The diagnostics should help distinguish stale Linear attachment URLs, missing managed markdown artifacts, and other mismatches between local backlog state and attachment-context download behavior.

## Acceptance Criteria

- A regression test reproduces the current listener startup failure/signal for a Todo issue in the listen pickup path.
- The reproduction distinguishes at least these cases: local backlog present, Linear attachments present but not downloadable, and no downloadable attachment context available.
- The failing summary/status strings used by the listener are identified and covered by test assertions so later fixes can safely change them.
- For Todo issues where local backlog context exists and attachment downloads fail or are unavailable, `meta agents listen` still starts the worker and does not emit a failure-style pickup summary.
- Listener behavior remains unchanged for truly blocking bootstrap failures such as workspace or workpad creation errors.
- Integration tests cover successful pickup with local backlog plus missing or non-downloadable attachment context and verify the final session summary text.
- When attachment-context download/setup is partial or empty, the listener records a concise machine-readable or markdown diagnostic that identifies the specific attachment(s) or file(s) involved.
- The dashboard/session summary stays compact and does not expand into per-file error spam.
- Tests verify that a failed download leaves behind actionable diagnostics that include the relevant attachment title or path.
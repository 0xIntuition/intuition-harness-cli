# Fix shared TUI input behavior and standardize loading, cursor, and active-state cues across interactive dashboards

## Summary
The shared TUI input implementation in `src/tui/fields.rs` is the root of the current dashboard editing problems, and the surrounding plan/technical visuals do not apply a consistent interaction language. Inputs in the dashboard do not respond reliably, pasted or typed newlines get flattened into combined text, the visible cursor can diverge from the actual edit position, and loading or active states are harder to scan than they should be.

## Scope
- Audit every interactive TUI form that uses `InputFieldState` in the dashboards
- Replace the current inline `|` caret rendering approach with a consistent cursor model that does not show a duplicate box/line artifact
- Support expected multiline entry where the screen is presenting freeform text input
- Make cursor placement, selection-adjacent rendering, and end-of-line behavior deterministic for wrapped content and unicode-safe cursor movement
- Reuse the existing loading indicator pattern beyond the current limited usage in plan/technical flows where it improves clarity
- Standardize color and emphasis for active items, focused questions, status messages, and progress boxes in the TUI dashboards
- Make plan and technical dashboards use the same state vocabulary for loading, active, ready, and error conditions where practical
- Add focused tests around editing, pasting, wrapping, cursor positioning, and updated visual states using the existing ratatui test patterns

## Out of scope
- Browser dashboard work in `src/listen/web.rs`
- Large information-architecture changes to config/setup layout
- Planning-flow semantics for skipping empty questions

## Acceptance Criteria

- All interactive TUI forms that rely on `src/tui/fields.rs` accept typed input reliably in the active field.
- Multiline-capable fields preserve intended line breaks instead of collapsing pasted or typed newlines into one combined line.
- The rendered cursor no longer shows duplicate caret artifacts, and the visible cursor position matches the actual insertion point at line ends and wrapped boundaries.
- Plan and technical TUI dashboards use a consistent loading treatment and no longer mix unrelated spinner or status styles for similar states.
- The active question or active selection is visually distinct in progress/sidebar panels through consistent emphasis and color choices.
- Status, loading, and error cues are applied consistently across the affected TUI dashboards and remain legible in terminal snapshots.
- Targeted ratatui, unit, or snapshot tests cover cursor movement, insertion, backspace, paste handling, wrapped or multiline rendering regressions, and the key active/loading/error visual states updated by this cleanup.

## Definition of Done

- [x] Shared input rendering no longer injects a fake caret character into field content.
- [x] Multiline fields preserve newline boundaries in paste and submit paths where the screen expects freeform text.
- [x] Wrapped cursor placement is computed from shared cursor metadata rather than ad hoc per-screen math.
- [x] `meta plan` and `meta technical` loading panels use the same title/copy/state language.
- [x] Active/focused panels in plan/technical snapshots are visually emphasized consistently.
- [x] Focused tests for shared fields and touched dashboards pass locally.

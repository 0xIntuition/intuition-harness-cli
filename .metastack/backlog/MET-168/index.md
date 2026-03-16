# Redesign config and setup TUI layout so sidebar content stays readable without overflow

## Summary
The config and setup dashboards currently use narrow fixed sidebars that cause labels and option text to overflow or become hard to read. This needs a broader TUI layout pass rather than a narrow truncation fix.

## Scope
- Rework `src/config_command.rs` and `src/setup.rs` dashboard layouts so question lists, summaries, and controls remain readable at practical terminal sizes
- Reduce or remove hard-coded narrow sidebar widths that cause option text to escape the container
- Improve how long labels, selected values, and explanatory text wrap within the layout
- Preserve the current command behavior while making config/setup easier to scan end-to-end
- Add render-once or snapshot coverage for representative narrow and standard terminal sizes

## Out of scope
- Browser dashboard work
- Shared text-editing engine changes except where integration adjustments are required

## Acceptance Criteria

- `meta runtime config --render-once` and `meta runtime setup --render-once` render dashboards where sidebar and summary content stays inside its container at representative terminal widths.
- Long config/setup option labels and selected values remain readable through improved layout and wrapping instead of spilling outside the panel.
- The revised layout preserves access to all existing config/setup questions and controls without removing information.
- Snapshot or render-once tests cover at least one constrained-width case that previously overflowed.
# Allow empty follow-up answers to skip planning questions and continue the TUI planning flow

## Summary
The planning dashboard currently blocks the user when they press Enter on an empty follow-up answer. That behavior should change so an empty answer can intentionally skip the current question and continue through the planning workflow.

## Scope
- Update the planning question-step behavior in `src/plan.rs` so Enter on an empty answer advances instead of forcing an error
- Preserve clear progress tracking so skipped answers remain distinguishable from answered ones
- Ensure generated planning prompts and review state handle skipped answers intentionally rather than treating them as broken input
- Add tests for mixed answered and skipped question sequences

## Out of scope
- General cursor/input rendering bugs
- Config/setup layout redesign

## Acceptance Criteria

- In the planning questions dashboard, pressing Enter on an empty active answer advances to the next unanswered question instead of showing a blocking validation error.
- Skipped follow-up questions are represented intentionally in the planning flow so progress and downstream plan generation remain deterministic.
- The plan-generation path still works when the session includes a mix of answered and skipped follow-up questions.
- Automated tests cover empty-answer skip behavior and a mixed answered/skipped planning session.
# Standardize TUI multiline inputs to submit on Enter and newline on Shift+Enter

## Problem
Several interactive forms still treat `Enter` as newline-first behavior in multiline fields, which slows down common submit flows and is inconsistent across the repo.

## Scope
Update the shared TUI input behavior and every interactive form that uses multiline text so `Enter` submits the current form/action and `Shift+Enter` inserts a newline instead. This includes planning inputs, issue create/edit descriptions, cron prompt fields, and any other multiline TUI editor backed by the shared input components.

## Notes
This should include help text, keyboard hints, and regression tests so the behavior is explicit and consistent.

## Acceptance Criteria

- All multiline TUI inputs in this repository submit the active form or advance the active step on `Enter`, while `Shift+Enter` inserts a newline without submitting.
- Interactive flows that currently use multiline fields, including `meta backlog plan`, Linear issue create/edit forms, and cron prompt editing, have updated keyboard help text and tests covering the new behavior.
- Single-line inputs keep their existing submit/navigation behavior and paste handling remains unchanged apart from the new multiline submit semantics.
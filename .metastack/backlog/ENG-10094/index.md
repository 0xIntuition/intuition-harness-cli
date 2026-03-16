# Fix `meta backlog plan` session state loss after follow-up submission and generation

## Problem
The interactive planning flow can sometimes accept a follow-up answer, generate output, and then return the user to the initial request step instead of keeping them in the in-progress plan/review state.

## Scope
Reproduce the state-machine failure in `meta backlog plan`, fix the transition so request, follow-up answers, and generated review data persist correctly across async loading boundaries, and add regression coverage for the failing path.

## Notes
This issue is only for the planning command; do not broaden it to unrelated step-based TUIs unless the same root cause is proven shared.

## Acceptance Criteria

- Submitting follow-up answers in `meta backlog plan` no longer returns the user to the initial request step after generation completes; the session advances to the correct next stage with prior input preserved.
- If generation or revision fails, the plan session restores the correct prior stage with the user's existing request and answers intact instead of resetting to a fresh request form.
- Tests reproduce the reported regression path and cover both successful generation and error recovery across the planning session's async state transitions.
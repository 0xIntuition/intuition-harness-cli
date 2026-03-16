# Add composable automation steps so one job can run commands, workflows, and agents in a controlled sequence

## Problem
The current runtime is hard-coded to run a shell command first and then an optional agent prompt in the same working directory. That is too narrow for the workflows you described, especially because this repository already has a reusable workflow system under `meta agents workflows` that cron cannot call as a first-class primitive.

## Proposed change
Replace the fixed command-plus-agent execution model with an ordered step runner. A job should be able to define multiple steps such as shell command execution, workflow playbook execution, direct agent invocation, and reusable context or prompt preparation.

## Scope
- Introduce typed step kinds with shared env/context passing between steps.
- Allow jobs to call existing workflow playbooks as a first-class step instead of shelling out through ad hoc commands.
- Define step failure behavior, timeouts, and whether later steps run after failures.
- Preserve useful environment variables and logs for each step.

## Acceptance Criteria

- A single automation job can define and execute multiple ordered steps instead of only one shell command and one optional agent prompt.
- One supported step kind invokes an existing workflow playbook from the repo's workflow library without requiring a raw shell wrapper.
- Step outputs needed by later steps are surfaced through a documented shared context or environment contract.
- Runtime logs and status output show per-step start, finish, success/failure, and timeout information for a job run.
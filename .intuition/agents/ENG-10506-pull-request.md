# ENG-10506: Add `agents build` command — interactive headless agent loop for workspace QA

## Summary
- Linear issue: https://linear.app/0xintuition/issue/ENG-10506/add-agents-build-command-interactive-headless-agent-loop-for-workspace
- Published automatically by `intu agents listen` for `ENG-10506`
- Latest listener review: `agents build` implementation and focused validation are complete; listener progress now reads from the managed backlog block instead of backlog packet templates.

## Lifecycle
- Initial publication uses a draft PR for unattended work in progress.
- The same PR is promoted to ready for review during the existing review handoff.

## Completed In This Branch
- Changed `src/build.rs`
- Changed `src/cli.rs`
- Changed `src/lib.rs`
- Changed `src/config.rs`
- Changed `src/config_resolution.rs`
- Changed `src/listen/mod.rs`
- Changed `tests/build.rs`
- Changed `tests/commands.rs`
- Changed `README.md`
- Changed `WORKFLOW.md`
- Changed `.intuition/backlog/ENG-10506/index.md`
- Changed `.intuition/backlog/ENG-10506/specification.md`
- Changed `.intuition/backlog/ENG-10506/validation.md`
- Taught listen backlog-progress tracking to read the managed `metastack-listen-progress` block from `index.md`.
- Kept placeholder `Follow-up action`, `Task`, and `Criterion` checklist items from surfacing as real remaining work.

## Remaining Work
- No remaining implementation work is tracked locally.

## Issue Context
# Add `agents build` command — interactive headless agent loop for workspace QA

Introduce `meta agents build` as a lightweight, non-Linear-ceremony command that launches a headless agent with a user-provided prompt in a specific workspace directory and supports an interactive loop for iterative QA refinement plus mid-execution prompt continuation.

### CLI definition

* Add `Build(BuildArgs)` variant to `AgentsCommands` enum.
* `BuildArgs`: positional `workspace` (ticket ID or path), optional positional `prompt`, `--root`, `--agent`, `--model`, `--reasoning`, `--dir` (explicit path override), `--max-turns`, `--no-interactive`.
* Wire dispatch through `run_agents_command()` to a new `run_build()` handler.

### Workspace resolution

* Accept a Linear ticket ID (e.g. `MET-45`) and resolve it to the sibling workspace path via the existing `sibling_workspace_root` / `ticket_workspace_root` helpers.
* Also accept an explicit directory path via `--dir` flag, bypassing ticket-based resolution.
* Validate the resolved directory exists and is a git repository before launching.

### Provider resolution

* Use the same `CLI override > route > repo > global` precedence chain as `agents execute` and `agents listen`.
* Register `agents.build` as a new route key so users can configure provider/model/reasoning defaults for this command via `meta runtime config`.

### Single-run execution

* Build the agent invocation using the existing `BuiltinProviderAdapter` trait and `AgentInvocation` pipeline.
* Set the agent working directory to the resolved workspace path.
* Pass the user prompt via the provider's transport (stdin for Codex, arg for Claude).
* Stream agent stdout/stderr to the user's terminal in real time (blocking, not background).
* Print a completion summary (success/failure, token usage if available) when the agent finishes.

### Interactive prompt loop

* After the agent completes a run, print a completion notification and present an input prompt (e.g. `build> `) for the next instruction.
* Each new prompt spawns a fresh agent run in the same workspace with the same provider/model configuration.
* The loop continues until the user sends an exit signal: empty input, `exit`/`quit`, or Ctrl+C.
* On Ctrl+C during an active agent run, gracefully terminate the agent subprocess and return to the prompt (do not exit the loop immediately).
* Maintain a lightweight in-memory session that tracks the workspace path, provider config, and run count for the loop duration.
* Print a short status line before each run showing the workspace, provider, and run number.
* If a prompt is provided as a CLI positional argument, use it for the first run and then enter the loop. If no prompt is provided, go directly to the interactive prompt.
* `--no-interactive` flag: run a single prompt and exit without entering the loop (for scripted usage).

### Mid-execution prompt continuation

* While the agent is streaming output, accept user input via a queuing mechanism.
* Show a visual indicator that input was queued (e.g. `[queued] your message here`).
* For Codex: capture the continuation/session handle from JSON output and use `exec resume <session_id>` with appended context when the user sends a mid-run prompt.
* For Claude or providers without native continuation: hold the queued prompt and prepend it as context to the next loop iteration's prompt. Log a note so the user understands the prompt will be applied on the next run.
* Continuation must stay within the same workspace directory and provider configuration as the parent loop session.
* Do not persist continuation handles beyond the current `agents build` session.

## Acceptance Criteria

* `meta agents build MET-45 "fix the auth bug"` resolves the workspace path from the ticket ID and launches a headless agent with the prompt in that directory
* `meta agents build --dir /path/to/workspace "fix the auth bug"` accepts an explicit directory path
* Agent stdout/stderr streams to the user terminal in real time
* Provider/model/reasoning resolution follows the existing precedence chain with `agents.build` route key
* `--agent`, `--model`, `--reasoning` CLI overrides work correctly
* Command exits with appropriate exit code on agent success or failure
* Error message shown if workspace directory does not exist or is not a git repo
* After agent completes, user is prompted for another instruction and can start a new agent run in the same workspace
* Empty input or `exit`/`quit` cleanly exits the loop
* Ctrl+C during an active agent run terminates the agent and returns to the prompt rather than exiting the loop
* Running without a positional prompt enters the interactive prompt directly
* `--no-interactive` flag runs a single prompt and exits
* Each loop iteration reuses the same workspace path and provider configuration
* Status line before each run shows workspace path, resolved provider, and run number
* rUser can type additional instructions while an agent is actively running; queued input is visually acknowledged with a `[queued]` indicator
* For Codex provider: queued prompt is delivered via `exec resume` continuation when supported
* For providers without native continuation: queued prompt is prepended as context to the next loop iteration with a logged note
* Continuation handles are not persisted beyond the current session
* Passes `cargo clippy --all-targets --all-features -- -D warnings`

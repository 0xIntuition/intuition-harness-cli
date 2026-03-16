# Add reusable long-running progress tracking and a live merge progress screen to `meta merge`

Create a shared progress/loading module in `src/` for long-running agent-backed CLI work, then teach `meta merge` to publish structured phase and substep updates into that framework. After the user selects PRs in the existing merge dashboard, keep the terminal in a live progress view instead of dropping straight to an opaque CLI stream. The implementation should cover both interactive and non-interactive flows, expose major merge phases plus finer-grained per-PR substeps when available, persist enough structured run-state data under `.metastack/merge-runs/<RUN_ID>/` to reconstruct success and failure progress, and document the updated `meta merge` experience in `README.md`.

## Acceptance Criteria

- A shared progress/loading component exists in `src/` and is usable by multiple commands instead of keeping separate copy-pasted loading UIs
- The shared API supports a high-level current phase label, an ordered list of phases or steps, and optional detail text for the active step
- Interactive `meta merge` keeps users in a loading/progress experience after PR selection until the command finishes or fails
- The progress view shows the current phase and active substep in plain language, including stages such as workspace preparation, creating the plan, applying selected PRs, validation, push, and publishing the aggregate PR
- `meta merge` execution publishes structured progress updates for the major phases: workspace preparation, plan generation, merge application, validation, push, and PR publication
- Per-PR merge work records finer-grained substeps when available, including which pull request is being applied and whether conflict assistance was invoked
- Run artifacts under `.metastack/merge-runs/<RUN_ID>/` include enough structured progress data to reconstruct what phase and substep the run reached in success and failure cases
- Non-interactive `meta merge --no-interactive` prints clear textual progress updates instead of relying on silent waiting between the start and final summary
- Automated tests cover at least one successful merge run and one failure path to prove the progress state advances and stops on the expected phase, and touched loading-style commands retain coverage proving the shared component output remains correct
- `README.md` documents the updated `meta merge` execution flow and tests or render-once coverage prove the loading screen content is visible and stable
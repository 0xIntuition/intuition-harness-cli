# Add optional per-ticket agent selection for `meta agents listen` pickups

Add an advanced listener option that prompts for agent selection when a new ticket is picked up. By default, listener behavior should remain automatic and use the resolved listen default agent, but when the advanced option is enabled the operator should be able to choose which agent runs the newly claimed task.

This work should define how the listener dashboard or pickup flow presents agent choices and how the selected agent is persisted into the launched worker invocation, while keeping the default unattended flow unchanged.

## Acceptance Criteria

- Listener config supports an advanced toggle that enables per-ticket agent selection on pickup while default behavior remains unchanged
- When the toggle is disabled, `meta agents listen` launches workers with the resolved listen route default automatically
- When the toggle is enabled, the listener presents available agent choices at pickup time and launches the worker with the selected agent
- The selected agent is recorded in listener state or session metadata so the active worker can be audited afterward
- Tests cover both modes: default automatic routing and interactive per-ticket selection
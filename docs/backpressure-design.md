# Backpressure Design for the Listen Agent Daemon

This document describes backpressure mechanisms for the `intu agents listen` daemon,
covering both **system backpressure** (flow control across sessions) and **context
backpressure** (token budget management within a single agent session).

---

## Background

The listen daemon polls Linear for Todo issues, spawns a worker subprocess per
issue, and runs agents in a multi-turn loop (default 20 turns). Today, the
system has implicit limits (turn caps, stalled-turn detection, timeouts,
file-based locks) but no explicit flow control when load exceeds capacity.

When a backlog contains 30 issues and the daemon picks them all up in a single
cycle, every one of those issues gets a concurrent worker process, each
invoking an LLM agent subprocess. This creates simultaneous pressure on:

- **CPU/memory** from parallel agent subprocesses
- **Disk** from workspace clones
- **API rate limits** on Linear and GitHub
- **Token budgets** from concurrent LLM invocations
- **Context windows** as individual sessions accumulate turn history

Backpressure gives the system a vocabulary for saying "slow down" at each of
these layers.

---

## 1. System Backpressure: Concurrency Semaphore

### Problem

`pickup_issue()` currently claims every pending issue that passes label/assignment
filters. With a large backlog this spawns unbounded worker subprocesses.

### Proposal

Add a `max_concurrent_sessions` setting (default: 4) that caps how many worker
subprocesses the daemon runs simultaneously. Pending issues beyond this limit
remain queued and are picked up as slots free.

#### Config surface

```rust
// config.rs
pub const DEFAULT_LISTEN_MAX_CONCURRENT_SESSIONS: usize = 4;

// InstallListenSettings
pub struct InstallListenSettings {
    // ... existing fields ...
    pub max_concurrent_sessions: Option<usize>,
}

// PlanningListenSettings
pub struct PlanningListenSettings {
    // ... existing fields ...
    pub max_concurrent_sessions: Option<usize>,
}
```

#### Resolution order

1. CLI flag `--max-concurrent N`
2. Repo-scoped `PlanningListenSettings.max_concurrent_sessions`
3. Install-scoped `InstallListenSettings.max_concurrent_sessions`
4. Built-in default (`DEFAULT_LISTEN_MAX_CONCURRENT_SESSIONS`)

#### Implementation sketch

In the daemon cycle (mod.rs), gate `pickup_issue()` calls behind a slot count:

```rust
// During each listen cycle:
let running_count = state.sessions.iter()
    .filter(|s| s.phase.is_active() && s.pid.is_some_and(|pid| pid_is_running(pid)))
    .count();

let available_slots = max_concurrent_sessions.saturating_sub(running_count);
let issues_to_pick_up = pending_issues.into_iter().take(available_slots);

for issue in issues_to_pick_up {
    pickup_issue(&issue, ...)?;
}
```

No OS-level semaphore is needed since the daemon already runs a single-threaded
poll loop that spawns subprocesses. The slot count is just an arithmetic guard.

#### Dashboard visibility

The TUI dashboard header should display slot utilization:

```
Sessions: 4/4 active | 8 queued
```

This gives operators immediate visibility into backpressure state.

---

## 2. Context Backpressure: Token Budget Awareness

### Problem

Each agent session can run up to 20 turns. As turns accumulate, the agent's
context window fills with prior tool calls, diffs, error logs, and reasoning.
The system tracks `TokenUsage` and `TurnTokenSnapshot` per turn, but doesn't
act on this data to prevent context degradation.

When cumulative input tokens approach the model's context limit, the agent
experiences:

- Attention dilution (early instructions deprioritized)
- Lost coherence across turns
- Hard failures if the limit is exceeded
- Wasted spend on context that's no longer actionable

### Proposal

Introduce **context pressure levels** derived from cumulative token usage, and
adjust session behavior at each level.

#### Pressure levels

```rust
// state.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextPressure {
    /// < 50% of context budget consumed. Normal operation.
    Normal,
    /// 50-75% consumed. Warn in dashboard, consider summarization.
    Elevated,
    /// 75-90% consumed. Reduce turn budget, request concise output.
    High,
    /// > 90% consumed. Final turn, then wrap up.
    Critical,
}

impl ContextPressure {
    pub fn from_token_usage(cumulative_input: u64, context_budget: u64) -> Self {
        let ratio = cumulative_input as f64 / context_budget as f64;
        match ratio {
            r if r >= 0.90 => Self::Critical,
            r if r >= 0.75 => Self::High,
            r if r >= 0.50 => Self::Elevated,
            _ => Self::Normal,
        }
    }
}
```

#### Context budget configuration

```rust
// config.rs
pub const DEFAULT_CONTEXT_BUDGET_TOKENS: u64 = 180_000; // conservative for 200K models

// InstallListenSettings
pub struct InstallListenSettings {
    // ... existing fields ...
    pub context_budget_tokens: Option<u64>,
}
```

#### Behavioral responses to pressure

| Pressure | Dashboard | Agent behavior | Turn budget |
|----------|-----------|---------------|-------------|
| Normal | Green indicator | Standard prompting | Full remaining turns |
| Elevated | Yellow indicator | Add "be concise" hint to continuation prompt | Full remaining turns |
| High | Orange indicator | Inject summarization checkpoint before next turn | Reduce remaining turns by 50% |
| Critical | Red indicator | Final turn with wrap-up instructions | 1 turn remaining |

#### Implementation sketch (worker turn loop)

```rust
// worker.rs - inside the turn loop, before agent invocation

let cumulative_input = session_tokens.input.unwrap_or(0);
let context_budget = resolved_context_budget; // from config resolution
let pressure = ContextPressure::from_token_usage(cumulative_input, context_budget);

match pressure {
    ContextPressure::Critical => {
        // Inject wrap-up directive into the continuation prompt
        // Set effective_max_turns = current_turn + 1
        log::warn!(
            "[{}] context pressure CRITICAL ({}/{}), requesting wrap-up",
            session.issue_identifier, cumulative_input, context_budget
        );
    }
    ContextPressure::High => {
        // Inject summarization checkpoint
        // Halve remaining turn budget
        log::warn!(
            "[{}] context pressure HIGH ({}/{}), injecting summary checkpoint",
            session.issue_identifier, cumulative_input, context_budget
        );
    }
    ContextPressure::Elevated => {
        // Add conciseness hint to prompt
        log::info!(
            "[{}] context pressure ELEVATED ({}/{})",
            session.issue_identifier, cumulative_input, context_budget
        );
    }
    ContextPressure::Normal => {}
}
```

#### Summarization checkpoint

At `High` pressure, before the next agent turn, the worker can:

1. Write a summary of progress so far to the workpad
2. Clear the continuation handle (force a cold start)
3. Include the summary + remaining task in the new full prompt

This effectively "compresses" context by restarting the conversation with
a distilled state, similar to how humans take notes before starting a fresh
work session.

```rust
// Summarization checkpoint pseudocode
if pressure >= ContextPressure::High && session.latest_resume_handle.is_some() {
    let summary = extract_progress_summary(&session, &workspace)?;
    write_summarization_checkpoint(&workspace, &summary)?;

    // Clear resume handle to force cold start with summarized context
    session.latest_resume_handle = None;

    // The next turn will use FullPrompt mode with the summary injected
    // into the agent brief, avoiding the bloated continuation context
}
```

---

## 3. API Backpressure: Adaptive Polling

### Problem

The listen daemon polls Linear every 7 seconds (`DEFAULT_LISTEN_POLL_INTERVAL_SECONDS`).
When Linear returns errors, exponential backoff kicks in via `LinearFailureSnapshot`.
But there's no adaptive behavior in the success path -- the daemon polls at full
speed even when there's nothing to do.

### Proposal

Adapt the poll interval based on system load and recent activity:

```rust
fn adaptive_poll_interval(
    base_interval: Duration,
    running_sessions: usize,
    max_concurrent: usize,
    last_pickup_age: Duration,
    degraded: bool,
) -> Duration {
    if degraded {
        // Already handled by exponential backoff
        return base_interval;
    }

    let mut interval = base_interval;

    // At capacity: no point polling aggressively for new issues
    if running_sessions >= max_concurrent {
        interval = interval * 3; // 21s instead of 7s
    }

    // No pickups in a while: back off gently
    if last_pickup_age > Duration::from_secs(300) {
        interval = interval * 2; // 14s or 42s
    }

    // Cap at 60 seconds
    interval.min(Duration::from_secs(60))
}
```

This reduces unnecessary Linear API calls when the system is at capacity or
idle, preserving rate limit headroom for the calls that matter (sync, comments,
attachments).

---

## 4. Token Budget Backpressure: Global Spend Ceiling

### Problem

With multiple concurrent sessions, cumulative token spend can escalate quickly.
There's currently no mechanism to enforce a global budget across all active
sessions.

### Proposal

Add an optional `token_budget` configuration that pauses new session pickups
when cumulative spend in the current period exceeds a ceiling.

#### Config surface

```rust
// config.rs
pub struct ListenTokenBudget {
    /// Maximum cumulative input+output tokens across all sessions in a period.
    pub max_tokens_per_period: Option<u64>,
    /// Period duration in seconds (default: 3600 = 1 hour).
    pub period_seconds: Option<u64>,
}
```

#### Implementation

Track cumulative tokens across all sessions in `ListenState`:

```rust
// state.rs
pub(super) struct TokenBudgetTracker {
    pub period_start_epoch_seconds: u64,
    pub period_seconds: u64,
    pub cumulative_tokens: u64,
    pub max_tokens: u64,
}

impl TokenBudgetTracker {
    pub fn is_over_budget(&self, now: u64) -> bool {
        if now >= self.period_start_epoch_seconds + self.period_seconds {
            return false; // period expired, budget resets
        }
        self.cumulative_tokens >= self.max_tokens
    }

    pub fn record_turn(&mut self, tokens: &TokenUsage, now: u64) {
        // Reset if period expired
        if now >= self.period_start_epoch_seconds + self.period_seconds {
            self.period_start_epoch_seconds = now;
            self.cumulative_tokens = 0;
        }
        self.cumulative_tokens += tokens.total().unwrap_or(0);
    }
}
```

When over budget:
- **Running sessions continue** (don't kill mid-conversation)
- **New pickups are paused** until the period rolls over
- **Dashboard shows** "Token budget: 1.2M/1M (paused until 14:30)"

---

## 5. Resource Backpressure: Workspace Disk Awareness

### Problem

Each session creates a full workspace clone. On machines with limited disk,
many concurrent sessions could exhaust available space.

### Proposal

Before workspace creation in `ensure_ticket_workspace()`, check available disk:

```rust
// workspace.rs
pub const MIN_FREE_DISK_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GB

fn check_disk_pressure(workspace_parent: &Path) -> Result<bool> {
    let available = fs2::available_space(workspace_parent)?;
    Ok(available < MIN_FREE_DISK_BYTES)
}
```

If disk pressure is detected:
- Block the pickup with `BlockedCategory::Infra` and a clear message
- The session stays queued and retries next cycle
- Dashboard shows a disk pressure indicator

This is a simple safety valve rather than a sophisticated mechanism -- just
enough to prevent the daemon from filling the disk.

---

## 6. Dashboard Integration

All backpressure signals should be visible in the TUI dashboard. Add a
pressure summary row to the header:

```
+------------------------------------------------------------------+
| Listen  MET / CLI                                                |
| Sessions: 3/4 active | 5 queued                                 |
| Pressure: sessions OK | context 2/4 elevated | tokens 340K/1M   |
| Poll: 7s (adaptive: 21s, at capacity)                            |
+------------------------------------------------------------------+
```

The pressure bar provides at-a-glance system health without requiring
operators to drill into individual sessions.

---

## Implementation Priority

Ranked by impact-to-effort ratio:

| Priority | Mechanism | Impact | Effort |
|----------|-----------|--------|--------|
| 1 | Concurrency semaphore (Section 1) | High -- prevents thundering herd | Low -- arithmetic guard in pickup loop |
| 2 | Context pressure levels (Section 2) | High -- prevents degraded agent quality | Medium -- pressure enum + turn loop integration |
| 3 | Adaptive polling (Section 3) | Medium -- reduces wasted API calls | Low -- interval calculation function |
| 4 | Token budget ceiling (Section 4) | Medium -- cost control | Medium -- period tracker + state persistence |
| 5 | Disk awareness (Section 5) | Low (safety valve) | Low -- single check before workspace creation |

Sections 1 and 2 address the two most common failure modes: too many agents
at once (system) and individual agents running out of useful context (context).
Section 3 is a quick win. Sections 4 and 5 are optional safety nets.

---

## Relationship Between Mechanisms

```
                    Linear Backlog (N issues)
                            |
                    [Adaptive Polling] --- API pressure signal
                            |
                    [Token Budget Gate] --- spend ceiling
                            |
                    [Concurrency Semaphore] --- max_concurrent_sessions
                            |
                  +---------+---------+
                  |         |         |
              Session A  Session B  Session C
                  |         |         |
              [Context     [Context   [Disk
               Pressure]    Pressure]  Pressure]
                  |         |         |
              Turn loop  Turn loop  Blocked
```

Each layer is independent: the concurrency semaphore doesn't need to know
about context pressure, and context pressure doesn't need to know about the
token budget. They compose naturally because each operates at a different
granularity (daemon-wide, per-session, per-resource).

---

## Non-Goals

- **Distributed backpressure**: This design is single-daemon. Multi-daemon
  coordination (e.g., across machines) is out of scope.
- **Preemption**: Running sessions are never killed by backpressure. We only
  gate new pickups and adjust turn behavior.
- **Dynamic model switching**: Context pressure doesn't switch to a cheaper
  model mid-session. That's a routing concern, not a backpressure concern.

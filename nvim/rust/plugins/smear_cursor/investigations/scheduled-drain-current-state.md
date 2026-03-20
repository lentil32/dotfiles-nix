# Smear Cursor Scheduled Drain / Timer Current-State Investigation

Scope: current event-loop slowdown behavior in cleanup, scheduled drain, and timer handling.

## What This Note Covers

This note records the current scheduler-side slowdown model.

It is distinct from the renderer investigations above. The important result here is that current
slowdown is dominated by repeated scheduled edges and cleanup churn, not by one superlinear queue
algorithm.

## The Current Drain Model

Scheduled work is staged in a `VecDeque`-backed queue in
[`core_dispatch.rs`](../src/events/handlers/core_dispatch.rs#L197).

The drain budget is determined by thermal state in:

- [`scheduled_drain_budget`](../src/events/handlers/core_dispatch.rs#L585)
- [`scheduled_drain_budget_for_thermal`](../src/events/handlers/core_dispatch.rs#L615)
- [`scheduled_drain_budget_for_depth`](../src/events/handlers/core_dispatch.rs#L633)

For `Hot` and `Cold`, the queue drains half the backlog, clamped to `16..32` work units per edge.
For `Cooling`, it drains the entire pre-existing snapshot for that edge.

The actual drain loop is in
[`drain_scheduled_work_with_executor`](../src/events/handlers/core_dispatch.rs#L681).

## Complexity

Queue operations themselves are effectively amortized `O(1)` per staged or popped unit.

The scheduler-side burst cost is better described as:

```text
O(W + E)
```

Where:

- `W` = total staged work units executed
- `E` = scheduled drain edges needed to finish the burst

So the slowdown here is not "queue algorithm went quadratic." It is "the system needed too many
edges to converge while work kept being restaged."

## Why Bursts Feel Expensive

Fresh demand can re-enter cleanup as `Hot`, which keeps the drain path active across repeated
bursts. The relevant reducer-side cleanup progression is implemented in:

- [`policy.rs`](../src/core/state/policy.rs)
- [`support.rs`](../src/core/reducer/machine/support.rs#L265)
- [`observation.rs`](../src/core/reducer/machine/observation.rs#L107)

Because `Hot` only drains a bounded fraction per edge, a large or self-refreshing backlog can keep
rescheduling itself even though each individual queue operation is cheap.

## Timer Behavior: Current Code Vs Old Explanation

The current timer runtime is replace-and-cancel, not append-and-ignore.

Current behavior:

- [`set_core_timer_handle`](../src/events/runtime.rs#L230) replaces the outstanding timer handle
  for the same timer kind
- [`stop_core_timer_handle`](../src/events/runtime.rs#L238) stops the displaced host timer
- [`dispatch_shell_timer_fired`](../src/events/runtime.rs#L714) ignores a shell fire only when the
  handle is already gone

This means the current scheduler issue is not a buildup of intentionally stale timers.

## Why This Note Exists

An older explanation attributed slowdown to accumulated uncancelled timers. That does not match the
current runtime implementation anymore.

The current problem statement is:

- cleanup can remain thermally active across bursts
- scheduled work drains in bounded waves
- follow-up work can restage more drain edges

That is operational churn, not the worst asymptotic algorithm in the plugin.

## Bottom Line

The current scheduler-side slowdown is repeated hot-drain convergence, not timer accumulation and
not a superlinear queue algorithm.

Comment: if future telemetry still points at "timer scheduling regression," that label should be
interpreted as "too many scheduled edges under churn" unless the runtime timer ownership model
changes again.

# Smear Cursor Recovery Rewrite Plan

## Constraints

- [x] Assume no backward-compatibility requirements. Prefer simpler runtime semantics over preserving old behavior.
- [x] Preserve project rules: state transitions define truth, reducers stay pure, effects stay deferred and isolated, and failure or retry remains a typed lifecycle transition.

Comment: the slowdown is a coupled-system problem, not one bad knob. The current tail comes from three sources acting together: an oversized retained render pool, repeated delayed-ingress timer rearming, and conservative scheduled-drain behavior after the burst ends.

## Workstream 1: Thermal Lifecycle And Cleanup Contract

- [x] Replace implicit warm or settle behavior with explicit thermal states: `Hot`, `Cooling`, `Cold`.
- [x] Add reducer-owned cleanup state fields: `idle_target_budget`, `max_prune_per_tick`, `next_compaction_due_at`, and `entered_cooling_at`.
- [x] `Hot -> Cooling` when ingress has gone quiet long enough to stop optimizing for reuse.
- [x] `Cooling -> Hot` on fresh ingress.
- [x] `Cooling -> Cold` when the pool is compacted to `idle_target_budget`, no smear windows remain visible, and no cleanup work is pending.
- [x] Add `RenderCleanupExecution::CompactToBudget { target_budget, max_prune_per_tick }`.
- [x] On `Hot -> Cooling`, immediately clear prepaint and hide all visible smear windows.
- [x] Make `CompactToBudget` the primary recovery mechanism.
- [x] Keep hard purge only as a fallback safety net, not as the normal path to idle recovery.

## Workstream 2: Split Hot Reuse Policy From Idle Retention Policy

- [x] Keep the adaptive pool budget only for `Hot` reuse decisions.
- [x] Add a separate idle retention budget with a small default, likely `0..2`.
- [x] Stop using `cached_budget` as the source of truth for cleanup once the lifecycle has entered `Cooling`.
- [x] Remove the assumption that the hot adaptive floor must also define idle retention.
- [x] Revisit `max_kept_windows` after the rewrite and lower or delete it if the new lifecycle makes the current cap unnecessary.

Comment: keep `max_kept_windows` as the explicit peak simultaneous-window cap. Cooling and idle retention now converge through the separate adaptive and idle budgets, but one burst can still need more live windows in a single frame than we want to retain after cleanup.

## Workstream 3: Window Pool Hot-Path Rewrite

- [x] Replace scan-derived counts with exact maintained counters for total, available, in-use, visible, and reusable windows.
- [x] Keep full-vector scans only for invariant checks, recovery, and debug paths.
- [x] Add a generic reusable-window freelist for placement misses.
- [x] Keep the exact-placement index for placement hits.
- [x] Remove or sharply reduce linear reuse scans across retained windows.
- [x] Centralize counter, index, and freelist updates so prune, hide, close, invalidate, and reuse all mutate shared bookkeeping in one place.
- [x] Add invariant helpers and tests for counters, placement index correctness, freelist correctness, and snapshot correctness.

## Workstream 4: Delayed Ingress Timer Rewrite

- [x] Model delayed ingress as reducer-owned pending deadline state instead of repeated host timer replacement.
- [x] Extend ingress policy state with the minimum deadline state needed to describe one pending delayed ingress window.
- [x] Only arm the host ingress timer on the transition from no pending delayed ingress to pending delayed ingress.
- [x] While delayed ingress is already pending, update reducer state and coalesced cursor demand without emitting repeated `timer_stop` or `timer_start` churn.
- [x] On ingress timer fire, observe immediately if due; otherwise rearm once for the remaining delay.
- [x] Keep latest cursor-demand coalescing and let the already armed timer consume that latest state.
- [x] Remove unnecessary host timer replacement from the runtime scheduling path.

## Workstream 5: Cooling-Phase Drain Policy

- [x] Keep bounded scheduled-drain behavior in `Hot`.
- [x] Switch to aggressive convergence in `Cooling`: either drain to empty or use a much larger bounded budget with the same observable result.
- [x] Make queue convergence part of the lifecycle contract instead of a side effect of repeated dispatch edges.
- [x] Expose separate diagnostics for hot backlog, cooling backlog, delayed-ingress churn, and post-burst convergence time.

## Workstream 6: Harness, Diagnostics, Tests, And Dead-Code Sweep

- [x] Update `plugins/smear_cursor/scripts/perf_window_switch.lua` to measure recovery as "reach `Cold`" instead of "wait a fixed short settle interval".
- [x] Keep both fixed-settle and state-based perf modes so new regressions can be compared against old runs.
- [x] Add scenarios for heavy burst switching, delayed ingress enabled, delayed ingress disabled, short settle, and long settle.
- [x] Record these diagnostics in perf runs: thermal lifecycle state, pool size, idle target budget, compaction progress, host timer rearm count, delayed ingress pending state, and queue drain behavior by lifecycle.
- [x] Keep logging off by default for perf runs.
- [x] Document clearly that `logging_level = 4` is least verbose in this plugin, not most verbose.
- [x] Add unit tests for `Hot -> Cooling -> Cold` and `Cooling -> Hot`.
- [x] Add unit tests proving repeated cursor ingress does not repeatedly rearm the host timer.
- [x] Add unit tests proving `CompactToBudget` converges to `idle_target_budget`.
- [x] Add unit tests for pool counters, freelist, placement index, and snapshot invariants.
- [x] Add regressions for no stale visible smear windows after cooling, no oversized retained pool after convergence, no queue tail after idle convergence, and no stale timer-token churn after burst ingress.
- [x] Add property-style tests for bounded queue growth and timer-token correctness.
- [x] Remove dead code, compatibility shims, and config knobs that become meaningless after the rewrite.

Comment: the sweep removed the live scan-and-rebuild bookkeeping path and unused legacy Lua parse wrappers. No additional cleanup-era runtime knob survived beyond `max_kept_windows`, which still caps peak simultaneous-window demand.

## Primary Files

- [x] State and reducers: `plugins/smear_cursor/src/core/state/policy.rs`, `plugins/smear_cursor/src/core/reducer/machine/observation.rs`, `plugins/smear_cursor/src/core/reducer/machine/timers.rs`, `plugins/smear_cursor/src/core/reducer/machine/support.rs`, `plugins/smear_cursor/src/core/runtime_reducer/cleanup.rs`
- [x] Runtime and dispatch: `plugins/smear_cursor/src/events/runtime.rs`, `plugins/smear_cursor/src/events/handlers/core_dispatch.rs`, `plugins/smear_cursor/src/events/handlers/render_apply.rs`
- [x] Draw and pool: `plugins/smear_cursor/src/draw/mod.rs`, `plugins/smear_cursor/src/draw/window_pool/mod.rs`, `plugins/smear_cursor/src/draw/window_pool/ops/adaptive.rs`, `plugins/smear_cursor/src/draw/window_pool/ops/acquire.rs`, `plugins/smear_cursor/src/draw/window_pool/ops/cleanup.rs`, `plugins/smear_cursor/src/draw/window_pool/ops/snapshot.rs`
- [x] Config and perf harness: `plugins/smear_cursor/src/config.rs`, `plugins/smear_cursor/scripts/perf_window_switch.lua`, `plugins/smear_cursor/scripts/run_perf_window_switch.sh`

## Acceptance Gates

- [x] Under the default burst stress harness, the runtime reaches `Cold` without relying on the `3s` hard-purge fallback.
- [x] Under the default burst stress harness, the pool shrinks to `idle_target_budget` within `1000 ms` of the last ingress.
- [x] Under the default burst stress harness with `1200 ms` settle, `recovery_ratio <= 1.10`.
- [x] Under the default burst stress harness with `3500 ms` settle, `recovery_ratio <= 1.05`.
- [x] During one continuous delayed-ingress burst, host delayed-ingress timer rearms stay bounded to burst-level behavior and do not scale with ingress event count.
- [x] After `Cooling` completes, later switches no longer pay pool-size-dependent hot-path costs from stale retained windows.
- [x] Diagnostics are sufficient to explain future regressions without ad hoc tracing.

Comment: validated on 2026-03-19 with the default matrix harness. `burst_delay_on_cold` and `burst_delay_off_cold` both reached `Cold` in about `198 ms`; the retained pool converged to `0` windows for delay-on and `2` windows for delay-off, matching the `idle_target_budget = 2` cap. Fixed-settle recovery ratios stayed below the gates in every scenario: `1.039` and `0.978` at `1200 ms`, `0.977` and `0.727` at `3500 ms`. The delayed-ingress burst kept `host_timer_rearms_ingress = 0` even while `delayed_ingress_pending_updates` grew above `80k`, so timer churn stayed burst-level instead of scaling with ingress count.

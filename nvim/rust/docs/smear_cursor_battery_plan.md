# `smear_cursor` battery/perf enhancement plan (static analysis)

This checklist comes from static analysis only. I did **not** run `cargo`, benchmarks, or runtime profiling in this environment.

## Highest priority

- [ ] **Shrink the render-planning payload so the hot path stops cloning large state objects.**
  - [ ] Replace `RequestRenderPlanEffect { planning_state: CoreState, ... }` with a lean planning payload that owns only the data render planning actually needs (for example `SceneState`, proposal/protocol metadata, and any minimal observation/projection inputs) instead of a full `CoreState`. Evidence: `src/core/effect.rs:361-368`, `src/events/handlers/render_plan.rs:8-24`, `src/core/reducer/machine/planning.rs:643-668`.
  - [ ] Remove the redundant `observation: ObservationSnapshot` field from `RequestRenderPlanEffect`, or reduce it to just `ObservationId` for logging, because the render-plan handler currently uses the full snapshot only for trace output while `planning_state` already carries protocol observation state. Evidence: `src/core/effect.rs:361-368`, `src/events/handlers/render_plan.rs:10-23`.
  - [ ] Eliminate `planning_state.clone().into_planning(proposal_id)` by restructuring the ownership flow so the transition state and the effect payload do not both need an owned copy of the same planning state. Evidence: `src/core/reducer/machine/planning.rs:576-597`.

- [ ] **Stop cloning `RuntimeState` just to plan one transition.**
  - [ ] Rework `plan_runtime_transition()` so it can mutate owned runtime state or a smaller runtime-planning payload instead of starting with `let mut runtime = state.runtime().clone();`. Evidence: `src/core/reducer/machine/planning.rs:497-559`, `src/state/machine/mod.rs:21-39`.
  - [ ] Verify that particle-heavy runtime fields (`particles: Vec<Particle>`) and motion arrays are not copied when the reducer only needs to advance state once and hand it forward. Evidence: `src/state/machine/mod.rs:21-39`.

- [ ] **Stop cloning `SceneState` and `PlannerState` in the planner path.**
  - [ ] Avoid `let current_scene = state.scene().clone();` inside `update_scene_from_render_decision()` by letting render planning own/mutate the next scene directly instead of cloning the full current scene first. Evidence: `src/core/reducer/machine/planning.rs:340-428`, `src/core/state/scene.rs:496-505`.
  - [ ] Replace `planner_seed()`’s unconditional `entry.planner_state().clone()` with shared ownership or a split planner representation (`Arc`/copy-on-write for history/cache, separate mutable scratch/current step state). Evidence: `src/core/reducer/machine/planning.rs:104-118`, `src/draw/render_plan/infra/shared.rs:163-205`.
  - [ ] Remove the extra planner clone on the projection reuse path (`entry.planner_state().clone()`) by reusing planner cache/history through reference-counted storage rather than deep-cloning `latent_cache`, `center_history`, and `previous_cells`. Evidence: `src/core/reducer/machine/planning.rs:301-323`, `src/draw/render_plan/infra/shared.rs:163-205`.

- [ ] **Remove the duplicated runtime-planning pass for background probes.**
  - [ ] Stop calling `plan_runtime_transition()` inside `background_probe_plan_for_observation()` and then calling it again later from `plan_ready_state()`. Compute the background probe plan from the already-produced render frame / cursor transition, or thread the earlier transition forward. Evidence: `src/core/reducer/machine/observation.rs:280-297`, `src/core/reducer/machine/planning.rs:561-598`.
  - [ ] Keep this fix high priority for configurations with `particles_enabled=true` and `particles_over_text=false`, because background sampling is only requested in that case. Evidence: `src/config.rs:156-163`, `src/core/reducer/machine/support.rs:287-298`.

- [ ] **Stop copying the particle vector into a fresh `Arc<[Particle]>` every rendered frame.**
  - [ ] Replace `Arc::from(state.particles().to_vec())` with a shared particle representation that can move or share ownership without a full copy each frame (for example runtime-owned `Arc<[Particle]>`, double-buffered particle storage, or a render-frame borrow/model change). Evidence: `src/core/runtime_reducer/frame.rs:59-81`, `src/types.rs:192-238`, `src/state/machine/accessors.rs:141-143`.
  - [ ] Keep the zero-particle fast path allocation-free.

- [ ] **Rework the host timer bridge to reduce create/stop churn.**
  - [ ] Replace repeated one-shot `timer_start()` / `timer_stop()` cycles with a persistent per-kind timer or a host-side rearm/update scheme, so animation does not constantly allocate/replace timers. Evidence: `autoload/nvimrs_smear_cursor/host_bridge.vim:7-16`, `src/events/runtime/timers.rs:76-200`, `src/events/timers.rs:56-68`.
  - [ ] Collapse the timer callback bridge so timer fire does not need a Vimscript callback that re-enters Lua via `luaeval("require('nvimrs_smear_cursor').on_core_timer(_A)", ...)` on every fire. Evidence: `autoload/nvimrs_smear_cursor/host_bridge.vim:7-15`.

- [ ] **Reduce the default wakeup rate for battery-sensitive use.**
  - [ ] Consider lowering the default animation FPS from `144.0` and/or adding an adaptive draw cadence (for example 60/72 Hz during tail drain, idle settling, or when buffer perf class is not `Full`). Evidence: `src/config.rs:9`, `src/config.rs:173-175`.
  - [ ] Keep simulation quality separate from draw cadence where possible, so motion stays smooth without redrawing at the highest rate on every machine.

## Medium priority

- [ ] **Make cursor text-context sampling two-phase so pure cursor motion stops reading buffer lines unnecessarily.**
  - [ ] Read only `changedtick` first.
  - [ ] Skip `get_lines()` sampling when the previous and current `changedtick` are equal, because semantic classification already returns `false` in that case. Right now moving to a new line with the same `changedtick` can miss the cache and still read nearby lines even though `text_mutated_at_cursor_context()` will immediately short-circuit. Evidence: `src/events/handlers/observation/base.rs:132-139`, `src/events/handlers/observation/text_context.rs:55-100`, `src/core/state/observation/semantic.rs:37-57`.

- [ ] **Reduce observation snapshot churn during probe completion.**
  - [ ] Replace clone-and-return update patterns (`observation.clone().with_*`) with in-place mutation or a smaller mutable probe-state object, especially for probe-completion waves. Evidence: `src/core/reducer/machine/observation.rs:255-277`, `src/core/reducer/machine/observation.rs:306-513`.
  - [ ] Remove the extra observation clone in `complete_observation()` if the ready/planning transition can move the snapshot once instead of storing it and passing it separately. Evidence: `src/core/reducer/machine/observation.rs:436-453`.

- [ ] **Deduplicate particle cell aggregation work.**
  - [ ] Stop scanning the particle list once for background-probe planning and again for draw aggregation when both are enabled. Reuse one shared cell-aggregation result for both consumers. Evidence: `src/core/state/observation/background_probe.rs:47-70`, `src/draw/render/particles.rs:44-127`.
  - [ ] Replace `BTreeSet`/`BTreeMap` with reusable scratch maps where strict in-order insertion is not required; sort only at the final emission boundary if deterministic order is needed. Evidence: `src/core/state/observation/background_probe.rs:47-70`, `src/draw/render/particles.rs:68-85`.

- [ ] **Avoid redundant real-cursor highlight writes.**
  - [ ] Track whether the real cursor is already hidden/unhidden and skip repeated `api::set_hl()` calls when the requested visibility state has not changed. Evidence: `src/events/logging.rs:242-259`, `src/events/handlers/render_apply.rs:49-63`, `src/events/handlers/render_apply.rs:212-229`.

- [ ] **Tighten ingress wakeup gating before deeper work starts.**
  - [ ] Add an earlier no-op fast path for cursor/window autocmds when the tracked window/buffer/cursor tuple has not changed in a way that can affect smear output.
  - [ ] Re-check whether `WinScrolled`, `CursorMovedI`, and `BufEnter` always need a full observation path under current state/config, or whether some can stop after lightweight snapshot comparison. Evidence: `src/events/ingress.rs:13-77`, `src/events/handlers/ingress_router.rs:45-104`, `src/events/lifecycle.rs:85-100`.

- [ ] **Keep trail-plan reuse available when particles are present.**
  - [ ] Investigate splitting the trail signature from the particle overlay so `frame_draw_signature()` does not return `None` just because particles exist; particles are drawn after trail decode and may not need to invalidate the whole trail-plan reuse path. Evidence: `src/draw/render_plan/lifecycle.rs:3-29`, `src/draw/render_plan/lifecycle.rs:112-149`.

## Lower priority

- [ ] **Replace the small VecDeque LRU only if higher-priority fixes are done and profiling still points here.**
  - [ ] The current cache does linear search plus clone-on-get; capacities are small, so treat this as secondary. Evidence: `src/events/lru_cache.rs:10-58`, `src/events/probe_cache.rs:10-14`.

- [ ] **Use callback-based autocmd registration instead of command strings if the API supports it cleanly.**
  - [ ] The current setup routes every autocmd through a command string like `lua require('nvimrs_smear_cursor').on_autocmd('CursorMoved')`, which adds parse/dispatch overhead on every wakeup. Evidence: `src/events/lifecycle.rs:85-100`.

## Verification tasks after patching

- [ ] Add temporary counters around `RuntimeState`, `SceneState`, `PlannerState`, and `ObservationSnapshot` clone sites so the before/after change is measurable.
- [ ] Add temporary telemetry for host timer start/stop/fire counts per second.
- [ ] Add temporary telemetry for particle count, background-probe cell count, and time spent in particle aggregation.
- [ ] Compare idle CPU during smear tail at the default config before/after each high-priority patch.
- [ ] Compare cursor movement CPU with `particles_enabled=true, particles_over_text=false` before/after removing the duplicate background-probe planning pass.

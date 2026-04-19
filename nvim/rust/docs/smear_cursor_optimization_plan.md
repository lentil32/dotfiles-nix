# `smear_cursor` battery + allocation optimization plan

Static analysis only. I did **not** build or run the crate in this environment. This plan is based on source inspection plus the checked-in perf snapshots already in the repo.

## What looks hottest right now

The strongest checked-in signal is the particle-heavy long-animation snapshot in `plugins/smear-cursor/perf/long-animation-allocation-current.md`:

- `~455,353.869` allocation ops/sec
- `~167,451,029.485` allocated bytes/sec
- `~269` particle simulation steps over the baseline window
- `~238` particle aggregation calls over the same window
- `0` particle overlay refreshes in that capture

Implication: the current battery risk is mostly **hot-path allocation churn inside active animation / planner / particle materialization**, not background-probe metadata reads and not particle-overlay refresh reuse (at least for that recorded workload).

The checked-in validation snapshot also shows `0` buffer metadata / changedtick / editor-bounds / command-row reads in the measured scenarios, so those do **not** look like the current bottleneck.

The checked-in planner compile snapshot looks roughly competitive between `reference` and `local_query`, so I would **not** start by reworking planner compile selection.

---

## P0 — biggest likely wins first

### 1) Eliminate the transient per-slice `BTreeMap` microtile container

- [x] Refactor `src/draw/render/latent_field/materialize.rs` so `materialize_swept_occupancy_with_scratch(...)` no longer returns a fresh `BTreeMap<(i64, i64), MicroTile>` on every deposited slice.
- [x] Replace the temporary `BTreeMap` with a compact row-major `Vec<((i64, i64), MicroTile)>`, `CellRows<MicroTile>`, or another contiguous representation that can be iterated immediately.
- [x] Compute the `CellRect` bounding box during materialization instead of doing a second pass via `CellRect::from_microtiles(...)` in `src/draw/render/latent_field.rs`.
- [x] Add a direct insertion path in `src/draw/render/latent_field/store.rs` so `LatentFieldCache` can consume the emitted tiles without forcing a temporary tree-shaped container.
- [x] Remove or narrow `DepositedSlice.microtiles: BTreeMap<...>` in production paths, since `stage_deposited_samples(...)` in `src/draw/render_plan/solver/staging.rs` builds the slice, inserts it into the cache, and then drops it immediately outside tests.
- [x] Keep deterministic iteration order explicitly if tests rely on it; do not preserve `BTreeMap` purely by inertia.

Why this matters:

- `stage_deposited_samples(...)` materializes microtiles for **every** step sample and **every** tail band.
- `LatentFieldCache::insert_slice(...)` immediately iterates the `BTreeMap` and does not retain that container in production.
- A temporary `BTreeMap` is one of the most allocation-heavy representations you could choose for this path.

Files to touch:

- `src/draw/render_plan/solver/staging.rs`
- `src/draw/render/latent_field/materialize.rs`
- `src/draw/render/latent_field.rs`
- `src/draw/render/latent_field/store.rs`

Expected benefit: **very high** on particle-heavy animation.

---

### 2) Retain full sweep-materialization scratch across frames and steps

- [x] Move `SweepMaterializeScratch` out of the local stack in `src/draw/render_plan/solver/staging.rs` and retain it inside `PlannerState` or another long-lived planner scratch struct.
- [x] Stop constructing `let mut sweep_scratch = latent_field::SweepMaterializeScratch::default();` inside `stage_deposited_samples(...)`.
- [x] Extend the retained scratch to include the projection vectors currently allocated by `prepare_swept_occupancy_geometry(...)` in `src/draw/render/latent_field/materialize.rs` (`row_projections`, `col_projections`).
- [x] Refactor `prepare_swept_occupancy_geometry(...)` to fill reusable buffers rather than returning a fresh `SweptOccupancyGeometry` with owned `Vec`s.
- [x] Keep the existing semantics, but make the hot path look like “clear + refill buffers” instead of “allocate + drop buffers.”

Why this matters:

- `prepare_swept_occupancy_geometry(...)` currently allocates row/col projection vectors on every step.
- `materialize_swept_occupancy_with_scratch(...)` already has a scratch API shape, but only the interval vectors are reused inside the function call, not across planner frames.
- This is exactly the kind of allocation churn that turns into battery drain in long-running animations.

Files to touch:

- `src/draw/render_plan/solver/staging.rs`
- `src/draw/render_plan/infra/shared.rs`
- `src/draw/render/latent_field/materialize.rs`

Expected benefit: **high**, especially when combined with item 1.

---

### 3) Collapse particle aggregation + screen-cell derivation into one retained pipeline

- [x] Rework `aggregate_particle_cells(...)` in `src/types.rs` so it does not build a `HashMap`, then clone all values into a `Vec`, then sort that `Vec`, then wrap it in `Arc`, on effectively every particle-updating frame.
- [x] Add retained aggregation scratch to `RuntimeState` instead of using only the thread-local `PARTICLE_CELL_SCRATCH` map.
- [x] Generate `particle_screen_cells` in the same aggregation pass when `particles_over_text == false`, rather than doing a second `collect::<Vec<_>>()` pass in `aggregate_particle_screen_cells(...)`.
- [x] Benchmark at least two concrete retained-output designs before settling on one:
  - retained `Vec` + sort once per refresh
  - retained ordered sparse structure keyed by cell
- [x] Do **not** blindly swap `HashMap` for `BTreeMap` without measurement; the win here is likely from avoiding clone/sort/output churn, not from picking a different map by name.
- [x] Keep the `RuntimeState` cache invalidation logic in `src/state/machine/accessors.rs`, but reduce the amount of work each invalidation forces.

Why this matters:

- `shared_aggregated_particle_cells()` is called from `build_render_frame(...)`.
- In the long-animation snapshot, particle aggregation calls are nearly as frequent as particle simulation steps.
- The current aggregation path performs multiple container/materialization passes even though downstream consumers mostly want ordered cells and sometimes screen cells.

Files to touch:

- `src/types.rs`
- `src/state/machine/accessors.rs`
- `src/core/state/observation/background_probe.rs` (if screen-cell generation gets folded into aggregation)

Expected benefit: **high** on particle-heavy animation.

Benchmark note:

- Local ignored release benchmark on April 7, 2026 favored the retained indexed-`Vec` pipeline at `~86,999 ns/iter` over the ordered sparse `BTreeMap` baseline at `~109,973 ns/iter` on a 4,608-particle fixture.

---

### 4) Replace hot-path `mode: String` ownership with a compact mode class / flags

- [x] Change `StepInput.mode: String` in `src/types.rs` to a compact mode representation (`ModeClass`, bitflags, or equivalent).
- [x] Change `RenderFrame.mode: String` in `src/types.rs` the same way.
- [x] Remove `mode.to_string()` from `build_step_input(...)` and `build_render_frame(...)` in `src/core/runtime_reducer/frame.rs`.
- [x] Update `src/animation/corners_sim.rs` so the “insert-like” decision uses the compact mode representation instead of `is_insert_like_mode(&input.mode)` on an owned `String`.
- [x] Update `src/core/state/scene.rs`, `src/core/realization.rs`, and `src/draw/palette.rs` so they stop cloning the mode string through `CursorTrailGeometry`, `planner_frame()`, and `PaletteSpec`.
- [x] Keep the exact raw mode string only at the editor boundary if some infrequent code path still needs it.

Why this matters:

- `build_step_input(...)` is in the per-simulation-step hot path.
- `build_render_frame(...)` is in the per-render-frame hot path.
- The current design clones the mode into multiple owned layers even though the logic mostly needs simple mode-family predicates.

Files to touch:

- `src/core/runtime_reducer/frame.rs`
- `src/types.rs`
- `src/animation/corners_sim.rs`
- `src/core/state/scene.rs`
- `src/core/realization.rs`
- `src/draw/palette.rs`

Expected benefit: **medium**, but very clean and worth doing.

---

## P1 — structural wins after the hottest allocation churn

### 5) Split planner-only config from palette config

- [x] Stop rebuilding a planner-only `RenderFrame` with a brand-new `Arc<StaticRenderConfig>` inside `CursorTrailGeometry::planner_frame(...)` in `src/core/state/scene.rs`.
- [x] Introduce a dedicated planner config type that contains only fields actually used by the planner / render-plan code.
- [x] Remove the fake palette-related fields from `planner_static_config(...)` in `src/core/state/scene.rs` (`String::new()`, color fields set to `None`, etc.).
- [x] Keep `StaticRenderConfig` for palette / shell-facing rendering, but stop using it as a transport type for planner-only work.
- [x] Re-check `PaletteSpec::from_frame(...)` in `src/core/realization.rs` after the split so it only carries palette data, not planner baggage.

Why this matters:

- `planner_frame()` currently clones mode/Arcs and constructs a fresh `StaticRenderConfig` even though planner code only needs a subset of those fields.
- This is an avoidable ownership and allocation tax on projection work.

Files to touch:

- `src/core/state/scene.rs`
- `src/types.rs`
- `src/core/realization.rs`
- `src/draw/render_plan/*`

Expected benefit: **medium**, plus better architecture.

---

### 6) Avoid full realization rebuild when only particle overlay changes

- [x] Rework the reusable-projection path in `src/core/reducer/machine/planning.rs` so `replace_particle_cells(...)` does not automatically force a full `realize_logical_raster(...)` rebuild through `ProjectionSnapshot::new(...)` in `src/core/state/scene.rs`.
- [x] Split realized output into “static spans” and “particle spans,” or add an equivalent mechanism that preserves realized static spans when only particle cells changed.
- [x] If practical, add a direct particle-overlay projection path that returns particle `CellOp`s (or particle spans) without building a full `RenderPlan` first.
- [x] Keep in mind that the checked-in long-animation allocation snapshot recorded `particle_overlay_refreshes = 0`, so this item is a **real redundancy** but probably **not** the main explanation for that particular allocation report.

Why this matters:

- `LogicalRaster::replace_particle_cells(...)` already preserves the static logical segment.
- `ProjectionSnapshot::new(...)` immediately re-realizes the whole raster anyway.
- That means the current code partially preserves work at one layer, then throws the savings away at the next layer.

Files to touch:

- `src/core/reducer/machine/planning.rs`
- `src/core/state/scene.rs`
- `src/core/realization.rs`
- `src/draw/render_plan/lifecycle.rs` (if overlay projection gets a direct fast path)

Expected benefit: **medium to high** on overlay-refresh-heavy workloads.

---

### 7) Reduce realization-pipeline passes and per-span overhead

- [x] Teach `RealizationSpanBuilder` in `src/core/realization.rs` to reserve based on an estimate from the incoming op count.
- [x] Carry a rolling payload hash in `PendingSpan` so span finalization does not need to re-hash every chunk in a second pass.
- [x] Use an O(1) prehashed finalization path so completed spans can move their chunk buffer directly into the retained `Arc<[RealizationSpanChunk]>`.
- [x] Revisit whether `project_render_plan(...) -> LogicalRaster -> realize_logical_raster(...)` can be partially fused once items 5 and 6 land.
      Current conclusion: keep the boundary for now and defer fusion until item 6 lands, because overlay-only reuse still swaps particle cells at the `LogicalRaster` layer.

Why this matters:

- The current realization path allocates fresh vectors and performs at least one avoidable extra pass over chunk payloads.
- This is not the biggest issue, but it is real hot-path CPU work.

Files to touch:

- `src/core/realization.rs`

Expected benefit: **medium**.

---

### 8) Remove the double scheduling hop for timer callbacks

- [x] Change `lua/nvimrs_smear_cursor/host_bridge.lua` so the libuv timer callback does not `vim.schedule(...)` **and then** enter a Rust callback that calls `schedule_guarded(...)` again in `src/events/timers.rs`.
- [x] Keep exactly one main-thread handoff mechanism.
- [x] Decide explicitly which layer owns panic isolation and “main loop only” guarantees.
      Current decision: keep Rust `schedule_guarded(...)` as the single main-thread handoff and panic-isolation boundary, and let the Lua libuv callback invoke the Rust entrypoint directly.
- [x] Validate timer semantics manually after the change (animation ticks, cleanup ticks, cancellation races).
      Validated at the real Neovim host-bridge boundary via `plugins/smear-cursor/scripts/test_timer_bridge.sh`, extended to cover immediate fire, stop-before-fire, and same-slot rearm/cancel.

Why this matters:

- This creates redundant queueing and wakeup overhead on every timer fire.
- It will not show up as allocation churn, but it is still wasted work and wasted wakeups.

Files to touch:

- `lua/nvimrs_smear_cursor/host_bridge.lua`
- `src/events/timers.rs`

Expected benefit: **medium** CPU / wakeup improvement.

---

## P2 — worthwhile cleanup once the major churn is gone

### 9) Reuse `RenderStepSample` storage across frames

- [x] Stop allocating a fresh `Vec<RenderStepSample>` in `src/core/runtime_reducer/reducer.rs` for every animated frame.
- [x] Move a reusable sample buffer into `RuntimeState`, or use a small inline buffer for the common case where the sample count is low.
- [x] Revisit `RenderFrame.step_samples: Arc<[RenderStepSample]>` in `src/types.rs` once planner/render ownership is cleaner; current conclusion: keep the shared `Arc` boundary for now because render, planner, and scene snapshots still share the same step-sample slice, but reclaim the transient `Vec` scratch that feeds it.

Why this matters:

- This path is exercised on essentially every animated frame.
- It is not likely to explain the whole allocation snapshot by itself, but it is steady background churn.

Files to touch:

- `src/core/runtime_reducer/reducer.rs`
- `src/core/runtime_reducer/frame.rs`
- `src/types.rs`
- `src/state/machine/mod.rs` / `RuntimeState` storage

Expected benefit: **medium-low**.

---

### 10) Avoid cloning the whole conceal-region slice on partial cache extension

- [x] Replace `cached.regions().to_vec()` in `src/events/cursor/conceal.rs` with a retained mutable buffer or append-friendly representation.
- [x] Preserve the current full-hit fast path (`Arc::clone(cached.regions())`).
- [x] Keep the cache API deterministic and easy to reason about.

Why this matters:

- Partial cache reuse currently still clones the entire cached region list before extension.
- This is not the main battery sink, but it is unnecessary churn.

Files to touch:

- `src/events/cursor/conceal.rs`
- possibly `src/events/probe_cache/*`

Expected benefit: **low**, but it is clean debt to pay down.

---

---

## Things I would **not** optimize first

- Validation probe metadata reads: the checked-in `validation-counters-current.md` shows zeros in the measured scenarios.
- Window-pool cap size: the checked-in docs already justify the shipped `64` default, and this does not look like the current battery bottleneck.
- Planner compile-mode selection: the checked-in `planner-compile-current.md` does not show an obvious regression big enough to outrank the hot allocation churn above.

---

## Suggested patch order

- [x] Patch item 1 first (temporary microtile `BTreeMap` removal).
- [x] Patch item 2 next (retained sweep geometry + interval scratch).
- [x] Patch item 3 next (particle aggregation / screen-cell materialization collapse).
- [x] Patch item 4 and item 5 together (mode ownership + planner/palette config split).
- [x] Patch item 7 after that (realization micro-optimizations).
- [x] Patch item 6 next if your real workload has overlay refresh churn.
- [x] Patch item 8 next (timer double-schedule).
- [x] Finish with items 9–10.

---

## Validation checklist after each patch set

- [x] Re-run `plugins/smear-cursor/scripts/capture_long_animation_allocations.sh` and update `plugins/smear-cursor/perf/long-animation-allocation-current.md`.
- [x] Re-run `plugins/smear-cursor/scripts/compare_particle_toggle_perf.sh` and confirm the `particles_on` tax shrinks rather than moves somewhere else.
- [x] Re-run `plugins/smear-cursor/scripts/compare_planner_perf.sh` and make sure planner baseline latency does not regress materially while fixing allocations.
- [x] Re-run `plugins/smear-cursor/scripts/capture_validation_counters.sh` and confirm the probe-read counters stay effectively at zero.
- [x] Add one focused perf/assertion test for every structural fast path you introduce (especially items 1, 2, 3, 6).

---

## Concrete success targets

- [ ] Cut long-animation allocation ops/sec materially from the current `~455k/s`.
- [ ] Cut long-animation allocated bytes/sec materially from the current `~167 MB/s`.
- [x] Keep planner-heavy baseline latency roughly flat while doing so.
- [x] Preserve current visual semantics and deterministic tests.

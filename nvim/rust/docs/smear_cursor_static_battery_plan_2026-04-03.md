# `smear_cursor` static battery/perf enhancement plan (2026-04-03)

This plan comes from **static analysis only**. I did **not** run `cargo`, benchmarks, or runtime profiling in this environment.

The checklist below covers the **current** CPU/memory redundancies I found in the checked-in code, with patch directions you can follow.

## Highest priority

- [x] **Move buffer-perf particle suppression earlier so degraded buffers stop paying particle costs they immediately discard.**
  - **Why this is redundant now:** the runtime reducer still simulates particles and builds a particle-bearing frame before `suppress_particles_for_perf_class()` clears them for non-ornamental perf classes. That means degraded buffers can still pay particle simulation, particle aggregation, and frame-mutation work on the hot path.
  - **Strong evidence:**
    - `src/core/runtime_reducer/reducer.rs:603-618` simulates steps with normal particle settings.
    - `src/core/runtime_reducer/frame.rs:68-84` clones `state.shared_particles()` into the frame and aggregates them.
    - `src/core/reducer/machine/planning.rs:191-211` only suppresses particles **after** the frame already exists.
  - **Extra waste hidden in the current order:** `RuntimeState::take_particles()` uses `Arc::unwrap_or_clone(...)` (`src/state/machine/accessors.rs:145-146`). Because `build_render_frame()` already cloned the particle `Arc` into `frame.particles` (`src/core/runtime_reducer/frame.rs:69-82`), the suppression path can clone the entire particle vector just to throw it away.
  - **Patch direction:** thread `BufferPerfClass` or a simple `allow_particles: bool` into the runtime-reducer path, set `particles_enabled = false` before stepping, avoid emitting/retaining particles in degraded modes, and skip particle frame fields entirely when ornamentals are disabled.
  - **Expected win:** avoids wasted particle simulation, avoids full-vector clone-on-discard, avoids aggregation work, and reduces allocation churn on fast/skip buffers.

- [x] **Cache particle-derived artifacts in `RuntimeState` instead of rebuilding aggregated particle cells every render frame.**
  - **Why this is redundant now:** `build_render_frame()` recomputes aggregated particle cells on every frame even though particle mutation already happens in a small number of state-transition methods.
  - **Strong evidence:**
    - `src/core/runtime_reducer/frame.rs:68-84` always calls `aggregate_particle_cells(...)`.
    - `src/types.rs:254-282` aggregates with a scratch `HashMap`, clones values into a `Vec`, sorts it, and allocates a new `Arc<[...]>`.
    - Particle mutation sites are concentrated in `src/state/machine/lifecycle.rs:318-331`.
  - **Patch direction:** add a dirty-flagged cache to `RuntimeState` for `SharedAggregatedParticleCells` (and possibly other particle-derived artifacts), invalidate it only when particles change (`apply_step_output`, `apply_scroll_shift`, `take_particles`, reset paths), and let `build_render_frame()` reuse the cached result.
  - **Expected win:** removes per-frame `HashMap`/`Vec`/sort work while the animation is active.

- [x] **Split `LogicalRaster` into particle and static segments so particle-only refreshes stop copying the whole raster.**
  - **Why this is redundant now:** the reuse path correctly avoids replanning the trail, but it still rebuilds a combined cell array when only the particle overlay changes.
  - **Strong evidence:**
    - `src/core/reducer/machine/planning.rs:341-357` refreshes only particle cells on the reuse path.
    - `src/core/realization.rs:133-139` then allocates a new `Vec` and copies both `particle_cells` and all `static_cells` into one new buffer.
  - **Patch direction:** store `LogicalRaster` as `{ particle_cells, static_cells }` (or equivalent segmented storage) and merge only at the final application boundary if a flat slice is absolutely required.
  - **Expected win:** avoids `O(total_cells)` copy/reallocation on frames where only particle overlay content changed.

- [x] **Stop doing a second particle-derived pass/allocation for background probes.**
  - **Why this is redundant now:** after particle aggregation is already available on the frame, background-probe planning walks those cells again and allocates another vector.
  - **Strong evidence:**
    - `src/core/state/observation/background_probe.rs:47-64` iterates `frame.aggregated_particle_cells()` and collects a fresh `Vec<ScreenCell>`.
    - `src/core/runtime_reducer/frame.rs:81` has already built the aggregated cells.
  - `RuntimeState` now caches a shared particle `ScreenCell` view alongside the aggregated particle cells, `RenderFrame` carries that cached slice only when background probes can use it, and `BackgroundProbePlan` filters the cached `ScreenCell` slice instead of re-deriving cells from aggregated particles.
  - **Expected win:** less iteration and allocation when `particles_over_text = false`.

- [x] **Memoize or defer render signatures, especially the particle-overlay signature.**
  - **Why this is redundant now:** projection prep hashes the full trail inputs and the full particle-overlay inputs on every planning pass.
  - **Strong evidence:**
    - `src/core/reducer/machine/planning.rs:242-245` computes both signatures eagerly.
    - `src/draw/render_plan/lifecycle.rs:3-39` hashes all step samples for the draw signature.
    - `src/draw/render_plan/lifecycle.rs:43-62` hashes every aggregated particle cell for the overlay signature.
  - **Patch direction:** cache signatures on `RenderFrame`/geometry or on the cached particle artifact, fast-path the empty-particle case, and defer overlay-signature computation until it is actually needed for reuse-key comparison/storage.
  - **Expected win:** less per-frame hashing during long animations or large particle sets.

- [x] **Remove raw particle storage from `RenderFrame` / `CursorTrailGeometry` when downstream only needs emptiness or derived cells.**
  - **Why this is redundant now:** production code mostly uses `frame.particles` / `geometry.particles` only to check whether any particles exist, while actual rendering and background probes use `aggregated_particle_cells`.
  - **Strong evidence:**
    - `src/draw/render/particles.rs:13` uses `frame.particles.is_empty()` as a guard and then iterates `frame.aggregated_particle_cells()`.
    - `src/core/reducer/machine/planning.rs:205` checks `frame.particles.is_empty()`.
    - `src/core/state/scene.rs:82-99`, `131-149`, and `152-153` carry/cloned raw particles mainly so `requires_background_probe()` can test emptiness.
  - **Patch direction:** keep raw particles only in `RuntimeState` for simulation, replace frame/scene copies with `has_particles` or `particle_count`, and keep using aggregated/shared derived data for rendering/planning.
  - **Expected win:** lower retained memory and fewer unnecessary `Arc<Vec<Particle>>` lifetimes in frame/scene/projection objects.

## Medium priority

- [x] **Add a hot-path buffer metadata cache so ingress snapshots stop rereading `filetype`, `buftype`, `buflisted`, and `line_count` on every cursor callback.**
  - **Why this is redundant now:** ingress snapshot capture always resolves current buffer perf policy from live metadata, even though most cursor events happen in the same buffer with unchanged options.
  - **Strong evidence:**
    - `src/events/runtime/ingress_snapshot.rs:107-128` always calls `read_current_buffer_event_policy()` during capture.
    - `src/events/runtime/engine.rs:74-80` always calls `BufferMetadata::read(...)`.
    - `src/events/cursor/buffer_meta.rs:14-25` performs 3 option reads plus `line_count()`.
  - **Patch direction:** split metadata into cold fields (`filetype`, `buftype`, `buflisted`) and hot fields (`changedtick`, `line_count`), cache cold fields per buffer handle, refresh line count only when `changedtick` changes, and consider an `OptionSet` invalidation path for the cold fields.
  - **Expected win:** fewer Neovim API round-trips and fewer temporary string allocations during cursor motion.

- [x] **Capture a shared editor snapshot once per observation/probe wave instead of rereading mode/window/buffer/changedtick in each helper.**
  - **Why this is redundant now:** the observation path and cursor-color validation repeatedly fetch the same current-editor facts from Neovim.
  - **Strong evidence:**
    - `src/events/handlers/observation/base.rs:119-185` orchestrates observation collection.
    - `src/events/handlers/observation/base.rs:50-68` reads the current window for cursor position.
    - `src/events/handlers/observation/text_context.rs:80-87` reads current buffer + changedtick again.
    - `src/events/handlers/observation/cursor_color.rs:27-56` reads current window/buffer/changedtick again for the witness.
    - `src/events/handlers/observation/cursor_color.rs:155-189` rereads mode, cursor position, window, buffer, and changedtick again for validation.
    - `src/events/cursor/screenpos.rs:25-27` allocates a fresh mode `String` each time.
  - Observation-base collection now captures one shared editor snapshot for mode, viewport, current window, current buffer, and changedtick, and cursor-color probe validation reuses the same snapshot shape instead of restitching those reads helper-by-helper.
  - **Patch direction:** introduce a `CurrentEditorSnapshot`/`ShellReadSnapshot` carrying mode, window handle, buffer handle, changedtick, viewport, and possibly cursor position; thread it through `current_core_cursor_position`, `current_cursor_text_context`, `current_cursor_color_probe_witness`, and validation.
  - **Expected win:** fewer repeated Neovim reads and fewer transient allocations on the observation path.

- [x] **Reuse cached viewport / command-row state instead of rereading `lines`, `cmdheight`, and `columns` across ingress and draw checks.**
  - **Why this is redundant now:** command-row checks and viewport reads each perform their own global option lookups.
  - **Strong evidence:**
    - `src/events/cursor/screenpos.rs:396-405` reads `lines` and `cmdheight` for `smear_outside_cmd_row()`.
    - `src/draw/apply.rs:44-51` reads `lines`, `cmdheight`, and `columns` for `editor_bounds()`.
    - `src/events/handlers/ingress_router.rs:150-162` uses `smear_outside_cmd_row()` on the cursor hot path.
    - `src/events/handlers/render_apply.rs:242-247` reads `editor_bounds()` again to validate live viewport.
  - Shell state now caches shared viewport dimensions for `editor_bounds()` and command-row queries, setup/toggle warm that cache, and `OptionSet(cmdheight)` plus `VimResized` refresh it so ingress, observation, and render-apply all reuse the same state.
  - **Expected win:** fewer global option reads while animating.

- [x] **Add an early ingress fast path so disabled runtime state does not pay buffer-policy metadata reads.**
  - **Why this is redundant now:** cursor-event handling captures a full ingress snapshot before preflight decides whether the callback should be dropped.
  - **Strong evidence:**
    - `src/events/handlers/ingress_router.rs:236-245` calls `ingress_read_snapshot()` and only then checks the preflight result.
    - `src/events/handlers/ingress_router.rs:182-195` drops early when `snapshot.enabled()` is false.
    - `src/events/runtime/ingress_snapshot.rs:107-128` still does current-buffer policy reads during capture.
  - Ingress snapshot capture now leaves `current_buffer_event_policy` unresolved when the runtime is disabled, so installed callbacks still drop cheaply without paying the current-buffer metadata/policy read path.
  - **Expected win:** lower idle battery use when the feature is toggled off but callbacks remain installed.

## Lower priority / polish

- [x] **Reduce hot-path `String` churn for mode reads.**
  - **Why this is redundant now:** `mode_string()` allocates on each call.
  - **Strong evidence:** `src/events/cursor/screenpos.rs:25-27`.
  - Hot-path mode reads now keep Neovim's `ModeStr` until ownership is actually needed, so ingress, lifecycle, observation, and slow-render logging paths stop allocating transient `String`s just to pass `&str` inputs through.
  - **Expected win:** small allocation reduction; not first-order compared with particle and metadata work.

- [x] **Trim avoidable clone-on-dispatch / clone-on-cache-hit paths only after the bigger fixes land.**
  - **Why this is redundant now:** some support utilities still clone values on the hot path, but they look secondary versus the issues above.
  - **Strong evidence:**
    - `src/events/runtime/timers.rs:191-203` clones the event on the successful timer-dispatch path.
    - `src/events/lru_cache.rs:158-176` always clones values on `peek_cloned()` / `get_cloned()`.
  - Timer dispatch now rebuilds the timer-fired event only on the rare recovery path, and the hot `LruCache` call sites that store `Copy` values use dedicated `peek_copy()` / `get_copy()` accessors instead of cloning on cache hits.
  - **Patch direction:** pass ownership through where possible or specialize the hottest caches around `Arc` values.
  - **Expected win:** minor cleanup only.

## Validation tasks after patching

- [x] Add temporary counters around particle simulation, particle aggregation, and particle-overlay-only raster refreshes.
  - Added a dedicated `validation_counters()` report so baseline capture stays separate from the compact `diagnostics()` payload.
- [x] Add temporary counters for `BufferMetadata::read()` and `current_buffer_changedtick()` calls per second.
  - Baseline capture saved in `plugins/smear-cursor/perf/validation-counters-current.md`.
- [x] Add temporary counters for `editor_bounds()` / command-row reads during active animation.
  - Baseline capture refreshed in `plugins/smear-cursor/perf/validation-counters-current.md`.
- [x] Re-measure CPU on a particle-heavy config with a degraded `BufferPerfClass` after moving particle suppression earlier.
  - Captured in `plugins/smear-cursor/perf/particle-degraded-buffer-current.md`: with the `particles_on` preset, degraded `fast` averaged `1461.789us` baseline vs `1493.331us` for `full`, with zero probe fallback calls in both modes.
- [x] Re-measure CPU and allocations during long animations after caching particle-derived artifacts.
  - Captured in `plugins/smear-cursor/perf/long-animation-allocation-current.md`: with `long_running_repetition` plus particles enabled, the baseline averaged `1618.899us` with `946661.0` allocation ops and `330134624.5` allocated bytes across the measured animation window.
- [x] Re-measure cursor-motion CPU in a large buffer after the buffer metadata cache lands.
  - Refreshed `plugins/smear-cursor/perf/validation-counters-current.md`: `large_line_count` now averages `1227.200us` baseline, with `buffer_metadata_reads/s`, `editor_bounds_reads/s`, and `command_row_reads/s` all at `0.000` during the measured animation window.

## Notes on items that appear **already fixed** in the current code

These showed up in the older perf-plan doc, but the current tree already appears to have them addressed, so I would **not** spend patch time on them first:

- Persistent host timer slots already exist in `lua/nvimrs_smear_cursor/host_bridge.lua:34-67`.
- Real-cursor highlight writes are already guarded in `src/events/logging.rs:245-277`.
- The render-planning payload is already much leaner in `src/core/effect.rs:372-426`.
- Autocmd registration already uses callback-based bridging in `src/events/lifecycle.rs:92-105`.

## My static-analysis ranking

If you want the best battery/CPU payoff first, I would patch in this order:

1. Move perf-class particle suppression earlier.
2. Cache particle-derived artifacts in runtime state.
3. Stop copying the full raster on particle-only refresh.
4. Add the hot-path buffer metadata cache.
5. Share a single editor snapshot across observation/probe helpers.

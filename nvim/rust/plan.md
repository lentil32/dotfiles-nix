# Smear Cursor Perf Patch Plan

Source review: `docs/review.md`
Patch area: `plugins/smear_cursor`

## Scope

- [x] Keep the first pass focused on the top ROI items from the review:
  - sparse or bitpacked background probes
  - better cursor-color and conceal caching
  - render-planner scratch reuse and shared sweep generation
- [ ] Avoid mixing in unrelated cleanup outside `plugins/smear_cursor` unless the patch requires a workspace-level fix.
- [ ] Preserve current render behavior and probe invalidation semantics; if behavior changes, document it in the patch notes and tests.

## Baseline And Guardrails

- [ ] Capture the current baseline before changing hot paths:
  - `cargo test -p rs_smear_cursor`
  - `cd plugins/smear_cursor && scripts/run_perf_window_switch.sh`
  - `cd plugins/smear_cursor && scripts/compare_particle_probe_perf.sh HEAD`
- [ ] Keep the config levers from the review in mind while validating:
  - `particles_over_text = true` disables background sampling
  - avoiding `"none"` for `cursor_color` and `cursor_color_insert_mode` disables cursor-color probing

## 1. Background Probe Refactor

- [x] Audit the current full-viewport background probe path in:
  - `src/core/state/observation.rs`
  - `src/events/handlers/observation.rs`
  - `src/events/host_bridge.rs`
  - `autoload/rs_smear_cursor/host_bridge.vim`
  - `lua/rs_smear_cursor/probes.lua`
  - `src/draw/render/particles.rs`
  - `src/core/realization.rs`
- [x] Replace the viewport-wide background mask with a sparse request scoped to active smear or particle cells, or another equivalent witness-bounded shape.
- [x] Change the wire format from per-cell `bool` objects to a packed representation such as bytes or row bitmasks.
- [x] Make sure the reducer and realization code can consume the new sparse or packed payload without regressing correctness.
- [x] Revisit the projection-cache invalidation path so background probing does not disable cache reuse globally when the probe is local to the active cells.
- [x] Add or update tests around background chunk sequencing, ready-state transitions, and background-gated particle materialization.
- [x] Re-run perf checks after this step before stacking more changes.

## 2. Cursor-Color And Conceal Cache Work

- [x] Audit the hot cursor probe and conceal paths in:
  - `src/events/cursor.rs`
  - `src/events/probe_cache.rs`
  - `src/events/host_bridge.rs`
  - `autoload/rs_smear_cursor/host_bridge.vim`
  - `lua/rs_smear_cursor/probes.lua`
- [x] Change cursor-color probe results from `#RRGGBB` strings to a numeric color payload such as `u32`.
- [x] Collapse the hot cursor-color probe path to fewer bridge hops if possible.
- [x] Cache `nvim_get_hl()` results by highlight group and invalidate them with the existing colorscheme-generation hooks in `src/events/probe_cache.rs`.
- [x] Replace the tiny cursor-color cache with a real small LRU, roughly 16 to 32 entries.
- [x] Replace the single-line conceal cache with a multi-line LRU keyed by `(buffer, changedtick, line)`.
- [x] If the conceal path still dominates, cache prefix conceal deltas or screen-boundary cells so repeated `synconcealed` and `screenpos()` work drops.
- [x] Add or update tests for colorscheme invalidation, navigation thrash, conceal-cache hits, and cache eviction behavior.
- [x] Re-run tests and perf checks after this step.

## 3. Observation Reuse

- [x] Review duplicate observation reads in:
  - `src/events/handlers/observation.rs`
  - `src/events/cursor.rs`
- [x] Reuse the already-captured observation snapshot inside the same reducer wave where correctness allows.
- [x] Only re-read the minimal fields needed when a queue hop or stale witness check makes reuse unsafe.
- [x] Add a small cache for cursor text context keyed by `(buffer_handle, changedtick, cursor_line, tracked_line)` if the current `get_lines()` plus allocation path still shows up hot.
- [x] Add tests for stale-cache avoidance and same-wave reuse.

## 4. Render Planner And Sweep Allocation Churn

- [x] Audit the planner and latent-field hot paths in:
  - `src/draw/render_plan/lifecycle.rs`
  - `src/draw/render_plan/infra.rs`
  - `src/draw/render_plan/solver.rs`
  - `src/draw/render/latent_field.rs`
- [x] Introduce reusable scratch storage for planner decode, following the existing scratch reuse pattern already present in the latent-field path.
- [x] Replace hot-path `BTreeMap` accumulation with a cheaper scratch structure where possible, and sort only at the edge if deterministic order is still required.
- [x] Stop rebuilding swept occupancy three times per frame sample; compute shared sweep geometry once and materialize the sheath, core, and filament bands from that shared data.
- [x] Add or update tests that lock in render output and deterministic ordering where needed.
- [x] Re-run perf checks after this step.
  - `2026-03-24`: `cargo test -p rs_smear_cursor`, `cd plugins/smear_cursor && scripts/run_perf_window_switch.sh`, `cd plugins/smear_cursor && scripts/compare_particle_probe_perf.sh HEAD`
  - particle probe baseline vs `HEAD`: `probe_on` `1334.603us` vs `1343.333us` (`-0.65%`), `probe_off` `1801.049us` vs `1819.364us` (`-1.01%`)

## 5. Secondary Follow-Ups

- [x] Revisit whole-state cloning in:
  - `src/events.rs`
  - `src/events/runtime.rs`
  - `src/events/handlers/core_dispatch.rs`
  - `src/core/state/protocol.rs`
- [ ] If clone pressure is still meaningful, move hot reducer payloads toward in-place mutation or `Arc` or copy-on-write splits.
- [x] Trim apply-path overhead in `src/draw/apply.rs` by moving span hashes upstream and resolving the `luaeval` capability once during setup.
- [ ] Keep these as separate commits from the top-ROI work unless the refactor naturally overlaps.

## Done Criteria

- [x] Unit and integration tests for `rs_smear_cursor` pass.
- [x] The perf harness runs cleanly, or the patch notes clearly state why it could not be run.
- [ ] The final handoff includes before and after numbers for the changed probe path when measurement is available.
- [x] The patch is split into reviewable commits or at least reviewable logical chunks by subsystem.

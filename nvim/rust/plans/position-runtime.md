# Position Runtime Plan

This phase aligns the reducer and runtime layers with the observation model from [position-observation.md](/Users/lentil32/.nixpkgs/nvim/rust/plans/position-observation.md). It should start only after observation emits canonical position facts.

## Goal

Preserve the four distinct facts called out in [position-spec.md](/Users/lentil32/.nixpkgs/nvim/rust/plans/position-spec.md) while giving each one a single owner:

- newest observed cursor cell
- latest exact anchor
- runtime target
- rendered pose

## Progress

- [x] Phase complete

## Primary Modules

- `plugins/smear-cursor/src/core/reducer/machine/planning.rs`
- `plugins/smear-cursor/src/core/reducer/machine/support.rs`
- `plugins/smear-cursor/src/core/event.rs`
- `plugins/smear-cursor/src/core/state/ingress.rs`
- `plugins/smear-cursor/src/state/machine/lifecycle.rs`
- `plugins/smear-cursor/src/state/machine/transitions.rs`
- `plugins/smear-cursor/src/events/runtime/editor_viewport.rs`
- `plugins/smear-cursor/src/draw/render_plan/infra/shared.rs`
- `plugins/smear-cursor/src/draw/apply.rs`

## Work Items

### 1. Rename and tighten exact-anchor ownership

- [x] Rename `latest_exact_cursor_position` to `latest_exact_cursor_cell` across reducer state and tests.
- [x] Update planning logic so exact observations refresh the anchor.
- [x] Update planning logic so deferred and unavailable observations do not overwrite the anchor.
- [x] Make the selection logic match exhaustively on the observed-cell state instead of relying on sidecar flags.

### 2. Remove ambiguous demand shadow state

- [x] Delete `requested_target` from `ExternalDemandQueuedEvent` and `ExternalDemand`.
- [x] Update ingress, timer-interleaving, and reducer support tests to reflect the removal.

### 3. Unify runtime target mutation

- [x] Reshape the runtime target into one object that carries the discrete target cell, shape, surface identity, retarget-relevant geometry, and `retarget_epoch`.
- [x] Introduce a dedicated retarget-key value instead of recomputing the comparison shape ad hoc at multiple call sites.
- [x] Replace split mutation paths in state-machine lifecycle and transition code with one mutation boundary.
- [x] Define the retarget key once and reuse it consistently in runtime and tests.

### 4. Collapse viewport math to one owner

- [x] Make `EditorViewportSnapshot` the canonical owner of command-row math and viewport-bounds projection.
- [x] Remove duplicated command-row calculations such as `command_row_from_dimensions()`.
- [x] Replace `core::types::ViewportSnapshot` and draw-layer `Viewport` with the shared `ViewportBounds`.

### 5. Delete ad-hoc conversions

- [x] Remove conversion helpers that exist only because multiple position families survived into runtime code.
- [x] Eliminate one-off `CursorPosition -> Point` and tuple-to-point translation helpers once the runtime consumes `ScreenCell` and `RenderPoint` directly.

## Rewrite Constraints

- [x] Do not keep `requested_target` as telemetry, snapshot, or compatibility state in this rewrite.
- [x] Do not keep `ViewportSnapshot` or draw-layer `Viewport` as compatibility wrappers once `ViewportBounds` is available.
- [x] Do not keep split runtime-target mutation paths after the new target owner lands.

## Jane Street Gates

- [x] Keep planning and runtime transition code exhaustive over domain enums; no wildcard arms for position-state decisions.
- [x] Prefer type-level distinctions over flag combinations when they remove illegal runtime states without making the API harder to reason about.
- [x] Prefer small value types for cells, keys, and shape tags; borrow larger scene or surface snapshots instead of cloning them through planning.
- [x] Do not add ambiguous boolean parameters to target-update or viewport APIs. Encode policy and state with dedicated types.
- [x] Update `RuntimeState::debug_assert_invariants()` and `CoreState::debug_assert_invariants()` to cover the new target owner and exact-anchor rules.
- [x] Keep target retargeting logic factored around one owned key so equality and epoch behavior are reviewable and testable.

## Done Criteria

- [x] Planning code consumes the normalized observation cell model and exact-anchor rules without sidecar sync state.
- [x] Runtime target updates go through one mutation boundary.
- [x] `requested_target` no longer exists.
- [x] One command-row implementation remains, and all viewport bounds used by planning/draw code derive from it.
- [x] Runtime code no longer depends on legacy position types or ad-hoc conversions.
- [x] Runtime target equality and `retarget_epoch` behavior are expressed through one reviewable key type.

## Targeted Tests

- [x] Reducer planning tests for exact vs deferred observations and exact-anchor retention.
- [x] Runtime reducer tests for retarget epoch stability across scroll translation, window movement, resize, and true target changes.
- [x] Viewport-related tests proving command-row behavior has one owner and produces stable bounds across observation and render planning.
- [x] Tests that exhaustive position-state handling preserves semantics for exact, deferred, and unavailable observation inputs.
- [x] Trace or snapshot updates if runtime-facing telemetry names change as part of the rename/removal work.

## Phase Exit Notes

Do not merge runtime target cleanup with unrelated animation tuning. This phase is about ownership and deterministic state transitions; keeping behavioral changes narrowly scoped will make the exact-anchor and retargeting regressions reviewable.

I traced the whole position pipeline: ingress fast path → observation base collection → core planning → runtime target/current geometry → render/apply.

The main conclusion is:

You do **not** have “too many positions” in the abstract. You have **4 position-like facts that are genuinely distinct**, and then several accidental duplicates / mixed-owner structs around them.

The 4 distinct facts you should keep are:

1. **Observed cursor cell** — the newest host sample, which may be exact or deferred.
2. **Latest exact anchor** — the last host-proven exact cell, cached across deferred reads.
3. **Runtime target** — the cell/surface/shape the motion engine is aiming at.
4. **Rendered pose** — the current animated geometry on screen.

Those should remain separate.

What should be removed is the accidental duplication around them.

---

## 1. What is redundant or violating SSoT today

### A. `CursorLocation` is a kitchen-sink struct, not a single fact

`src/state/cursor.rs:5-15` defines `CursorLocation` with:

- surface identity: `window_handle`, `buffer_handle`
- buffer cursor facts: `top_row`, `line`
- horizontal viewport facts: `left_col`, `text_offset`
- window placement/size: `window_row`, `window_col`, `window_width`, `window_height`

That is not one semantic fact. It is at least four.

It gets worse in `src/events/handlers/viewport.rs:92-143`:

- `cursor_location_for_core_render()` reconstructs it from live reads
- and if nothing exists, it returns `CursorLocation::new(0, 0, 0, 0)`

That is a fake sentinel state, not a real cursor location.

For a Jane Street-level model, invalidity must be represented as `Option` / a sum type, not as “zero means fake”.

**Verdict:** SSoT violation and invalid-state encoding.

---

### B. Window/surface facts are owned three times

You currently have three representations of almost the same host surface facts:

- `CursorLocation` in `src/state/cursor.rs:5-15`
- `WindowSurfaceMetrics` in `src/events/handlers/viewport.rs:24-32`
- `ConcealScreenCellView` in `src/events/cursor/conceal.rs:49-58`

And you parse the same `getwininfo` payload in two separate places:

- `current_window_surface_metrics()` in `src/events/handlers/viewport.rs:40-55`
- `ConcealScreenCellView::capture()` in `src/events/cursor/conceal.rs:94-112`

That means the same semantic fact — the live window surface snapshot — is being re-read and re-shaped independently in different subsystems.

That is exactly the kind of thing that creates edge drift, especially because conceal correction and general cursor observation are timing-sensitive already.

**Verdict:** direct SSoT violation.

---

### C. The host read logic for cursor location is duplicated

You have two separate read paths that construct almost the same cursor/surface snapshot:

- `cursor_location_for_ingress_fast_path_with_handles()` in `src/events/handlers/viewport.rs:76-90`
- `cursor_location_for_core_render()` in `src/events/handlers/viewport.rs:92-143`

Both read:

- `line("w0")`
- `line(".")`
- `getwininfo`

Both then construct `CursorLocation`.

The second one additionally does layered fallback with tracked values and zero defaults.

This should be one read function with one fallback policy, not two sibling implementations.

**Verdict:** redundant logic and drift risk.

---

### D. Viewport / command-row math has multiple owners

You currently have all of these:

- `EditorViewport` in `src/events/runtime/editor_viewport.rs:7-52`
- `ViewportSnapshot` in `src/core/types.rs:370-379`
- `draw::render_plan::Viewport` in `src/draw/render_plan/infra/shared.rs` and `src/draw/apply.rs:50-56`

And the command-row formula appears in at least two places:

- `EditorViewport::command_row()` in `src/events/runtime/editor_viewport.rs:37-42`
- `command_row_from_dimensions()` in `src/events/cursor/screenpos.rs:319-322`

That is not a catastrophic semantic bug yet, but it is a clear derived-contract duplication. The formula should exist once.

**Verdict:** representation duplication with SSoT drift risk.

---

### E. The same “screen cell” is represented in four type families

Right now you have:

- local alias `type ScreenCell = (i64, i64)` in `src/events/cursor/mod.rs:19`
- local alias `type ScreenPoint = (f64, f64)` in `src/events/cursor/mod.rs:20`
- shared `crate::types::ScreenCell` in `src/types.rs:126-177`
- `core::types::CursorPosition { CursorRow, CursorCol }` in `src/core/types.rs:346-379`
- `Point` in `src/types.rs:73-98`

And then a pile of conversions:

- `screen_cell_to_point()` in `src/events/cursor/screenpos.rs:80-82`
- `point_from_cursor_position()` in `src/core/reducer/machine/planning.rs:65-70`
- `to_core_coordinate()` in `src/events/handlers/observation/base.rs:32-37`
- `ScreenCell::from_rounded_point()` in `src/types.rs:140-156`

This is too many isomorphic types for one concept.

Even worse, invariants are not centralized:

- `parse_screenpos_cell_from_dict()` accepts only `row > 0 && col > 0` in `src/events/cursor/screenpos.rs:58-69`
- `crate::types::ScreenCell::new()` enforces `>= 1` in `src/types.rs:133-138`
- but `to_core_coordinate()` accepts `0.0` in `src/events/handlers/observation/base.rs:32-37`
- and `CursorRow(pub(crate) u32)` / `CursorCol(pub(crate) u32)` can be constructed crate-wide without validation in `src/core/types.rs:346-367`

So the “screen cell is 1-based and positive” invariant is true in some places and false in others.

**Verdict:** strong SSoT / type-invariant failure.

---

### F. Exactness is modeled as a sidecar, not as part of the cursor sample

Today the cursor read semantics are split across:

- `BufferCursorRead` with `raw_position`, `resolved_position`, `raw_position_sync` in `src/events/cursor/screenpos.rs:104-112`
- a family of selection helpers in `src/events/cursor/screenpos.rs:115-187`
- `ObservationMotion.cursor_position_sync` in `src/core/state/observation/snapshot.rs:153-197`
- `ObservationSnapshot::exact_cursor_position()` in `src/core/state/observation/snapshot.rs:309-312`

The semantic fact is “what cursor cell did we observe, and how exact is it?”
That should be one typed value.

Right now the position is in one place, the exactness is in another place, and the raw/resolved choice is hidden behind multiple selectors.

**Verdict:** semantic fact split across multiple owners.

---

### G. `ExternalDemand.requested_target` is dead or at least semantically ambiguous

`requested_target` exists in:

- `ExternalDemandQueuedEvent` in `src/core/event.rs:28-36`
- `ExternalDemand` in `src/core/state/ingress.rs:22-66`

Boundary refresh fills it in `src/core/reducer/machine/support.rs:181-210`.

But planning does **not** use `demand.requested_target()`.
Planning uses `state.latest_exact_cursor_position()` in `src/core/reducer/machine/planning.rs:606-646`.

So today `requested_target` is not the authoritative runtime target, not the observation cell, and not the exact-anchor cache either. It is mostly carried and logged.

That is dead semantic shadow state.

**Verdict:** redundant field. Remove it, or rename it to a narrow hint and actually use it in one place only.

---

## 2. What should remain distinct

These are **not** duplicates and should not be collapsed:

### Observed cursor cell

This is the newest host sample, and it may be deferred-exact because conceal correction is still pending.

### Latest exact anchor

This is the last host-proven exact sample.
The current planning comment in `src/core/reducer/machine/planning.rs:623-627` is correct: it is a fallback anchor, **not** the primary motion target.

### Runtime target

This is the discrete target the motion engine is chasing.

### Rendered pose

This is `current_corners`, the live animated state.

If you collapse these, you lose either responsiveness or correctness.

---

## 3. The spec I would adopt

Below is the concrete ownership spec I would implement.

## 3.1 Shared position module

All position primitives live in one shared module, for example `src/position/`.

No layer-local isomorphic position types are allowed in `events`, `core`, or `draw`.

A concrete shape:

```rust
pub mod position {
    use std::num::NonZeroU32;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
    pub struct ScreenCell {
        row: NonZeroU32, // 1-based
        col: NonZeroU32, // 1-based
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
    pub struct BufferLine(NonZeroU32); // 1-based

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
    pub struct ViewportBounds {
        max_row: NonZeroU32, // inclusive
        max_col: NonZeroU32, // inclusive
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
    pub struct SurfaceId {
        window_handle: i64,
        buffer_handle: i64,
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct WindowSurfaceSnapshot {
        id: SurfaceId,
        top_buffer_line: BufferLine,
        left_col0: u32,
        text_offset0: u32,
        window_row1: NonZeroU32,
        window_col1: NonZeroU32,
        window_width_cells: NonZeroU32,
        window_height_cells: NonZeroU32,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum ObservedCell {
        Unavailable,
        Exact(ScreenCell),
        Deferred(ScreenCell),
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct CursorObservation {
        buffer_line: BufferLine,
        cell: ObservedCell,
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct RenderPoint {
        row: f64,
        col: f64,
    }
}
```

### Mandatory rules

- `ScreenCell` is the **only** integral editor-screen coordinate type.
- `RenderPoint` is the **only** continuous runtime/render coordinate type.
- `CursorPosition`, `CursorRow`, `CursorCol`, tuple `(i64, i64)` screen cells, and `(f64, f64)` screen points must go away.
- No zero sentinels. Absence is represented explicitly.

---

## 3.2 Canonical owners

For each semantic fact, exactly one retained owner exists.

### Fact: live editor viewport raw witness

Owner: shell cache `EditorViewportSnapshot`
Fields: `lines`, `cmdheight`, `columns`

### Fact: observation viewport bounds

Owner: `ObservationBasis.viewport: ViewportBounds`
Derived from `EditorViewportSnapshot::bounds()`

### Fact: observation-time window surface snapshot

Owner: `ObservationBasis.surface: WindowSurfaceSnapshot`

### Fact: observation-time cursor sample

Owner: `ObservationBasis.cursor: CursorObservation`

### Fact: latest exact anchor

Owner: `CoreStatePayload.latest_exact_cursor_cell: Option<ScreenCell>`

### Fact: runtime target

Owner: `RuntimeState.target: CursorTarget`

### Fact: rendered pose

Owner: `RuntimeState.current_corners: [RenderPoint; 4]`

### Fact: derived visual cursor cell

Owner: nobody
Derived from rendered pose + runtime target

That last point matters: derived values are not owners.

---

## 3.3 `ObservationBasis` should own semantic facts, not sidecars

Replace the current split:

- `cursor_position`
- `cursor_location`
- `cursor_position_sync` in `ObservationMotion`

with:

```rust
pub struct ObservationBasis {
    observed_at: Millis,
    mode: String,
    surface: WindowSurfaceSnapshot,
    cursor: CursorObservation,
    viewport: ViewportBounds,
    buffer_revision: Option<Generation>,
    cursor_text_context_state: CursorTextContextState,
}
```

And keep `ObservationMotion` for actual motion-side metadata only:

```rust
pub struct ObservationMotion {
    scroll_shift: Option<ScrollShift>,
}
```

Cursor exactness belongs to the cursor sample, not to motion metadata.

---

## 3.4 One host read for surface, one host read for cursor

### Surface read contract

There is exactly one function that parses live host surface state:

```rust
fn read_window_surface_snapshot(
    window: &api::Window,
    buffer: &api::Buffer,
) -> Result<WindowSurfaceSnapshot>
```

It is the only place allowed to:

- call `getwininfo`
- call `line("w0")`
- shape those values into retained semantic data

`WindowSurfaceMetrics` disappears.
`ConcealScreenCellView::capture()` disappears.
Conceal logic receives `&WindowSurfaceSnapshot` from the caller.

### Cursor read contract

There is exactly one function that reads the host cursor sample:

```rust
fn read_cursor_observation(
    window: &api::Window,
    mode: &str,
    policy: CursorReadPolicy,
    surface: &WindowSurfaceSnapshot,
) -> Result<CursorObservation>
```

Internally it may still do:

- raw `screenpos`
- conceal-adjusted resolution
- raw fallback during fast motion
- cmdline special handling

But none of those intermediate representations escape the function.

Public output is one value: `CursorObservation`.

---

## 3.5 Exact-anchor and motion-target rules

These rules are the heart of the position model.

### Exact anchor update rule

```rust
match observation.cursor.cell {
    ObservedCell::Exact(cell) => latest_exact_cursor_cell = Some(cell),
    ObservedCell::Deferred(_) | ObservedCell::Unavailable => {
        // keep previous exact anchor
    }
}
```

This preserves the current good behavior.

### Motion target selection rule

```rust
let observed_cell = observation.cursor.cell.some_cell();
let fallback_anchor = latest_exact_cursor_cell;
let event_cell = observed_cell
    .or(fallback_anchor)
    .unwrap_or(runtime.target.cell());
```

### Normative behavior

- If the newest observation has a deferred cell, motion follows that deferred cell immediately.
- The exact anchor does **not** overwrite it.
- The exact anchor remains cached until a later exact refresh lands.

That keeps motion responsive while preserving exact resync.

---

## 3.6 `requested_target` should not exist as semantic state

Preferred spec:

- remove `ExternalDemand.requested_target`
- remove `ExternalDemandQueuedEvent.requested_target`

Boundary refresh does not need to carry a shadow target field if planning already uses `latest_exact_cursor_cell`.

If you keep a field for debugging or tracing, it must be renamed to something narrow like:

- `exact_anchor_hint`

and it must be explicitly classified as telemetry / snapshot-only, not semantic truth.

As implemented today, `requested_target` is a ghost field.

---

## 3.7 Runtime target should be one object, not split mutation paths

Today target mutation is split across:

- `set_target()` in `src/state/machine/transitions.rs:82-90`
- `update_tracking()` in `src/state/machine/lifecycle.rs:359-368`

Both can bump `retarget_epoch`.

That is serviceable, but not clean.

The runtime should own one target object:

```rust
pub struct CursorTarget {
    cell: ScreenCell,
    shape: CursorShape,
    surface: WindowSurfaceSnapshot,
    retarget_epoch: u64,
}
```

And one mutation boundary:

```rust
fn apply_target_snapshot(&mut self, next: CursorTarget)
```

### `retarget_epoch` rule

`retarget_epoch` increments exactly once when the **retarget key** changes.

The retarget key is:

- target cell
- target shape
- `surface.window_handle`
- `surface.buffer_handle`
- `surface.window_width`
- `surface.window_height`

It intentionally does **not** include:

- `top_buffer_line`
- `left_col0`
- `text_offset0`
- `window_row1`
- `window_col1`

Those are view translations, not retarget discontinuities.

This matches the current intent: scroll/viewport motion translates; window/buffer/discrete resize retargets.

---

## 3.8 Viewport math has one implementation

The canonical owner of raw editor viewport data is `EditorViewportSnapshot`.

It exposes one formula:

```rust
fn command_row(self) -> NonZeroU32 {
    max(1, lines - max(cmdheight, 1) + 1)
}
```

And one projection:

```rust
fn bounds(self) -> ViewportBounds {
    ViewportBounds {
        max_row: command_row(),
        max_col: max(columns, 1),
    }
}
```

Rules:

- `command_row_from_dimensions()` is deleted.
- `ViewportSnapshot` and `draw::render_plan::Viewport` either collapse into `ViewportBounds`, or become thin wrappers with no independent math.
- No second constructor may recompute `max_row`/`max_col`.

---

## 3.9 Conversion rules

Allowed conversions:

- `ScreenCell -> RenderPoint`
- `RenderPoint -> Option<ScreenCell>` only by explicit rounding at render-derived boundaries
- `EditorViewportSnapshot -> ViewportBounds`
- `WindowSurfaceSnapshot -> SurfaceId`
- `CursorTarget -> target_corners()`

Forbidden conversions:

- ad-hoc `f64 -> u32` cursor coordinate helpers in core
- tuple alias to point conversions
- layer-local cell structs that shadow the canonical one

Concretely:

- `to_core_coordinate()` should disappear
- `point_from_cursor_position()` should disappear
- local `screen_cell_to_point()` should disappear

---

## 3.10 No invalid sentinel states

This is mandatory.

The following pattern is forbidden:

- fake zero handles
- fake zero rows/cols
- `CursorLocation::new(0, 0, 0, 0)` style defaults

Absence is represented by:

- `Option<WindowSurfaceSnapshot>`
- `ObservedCell::Unavailable`
- runtime lifecycle state that says “uninitialized”

not by fake numeric values.

---

## 4. Concrete delete / merge list

I would remove or merge these exact items.

### Delete

- `events::cursor::ScreenCell` tuple alias in `src/events/cursor/mod.rs:19`
- `events::cursor::ScreenPoint` tuple alias in `src/events/cursor/mod.rs:20`
- `core::types::CursorRow`
- `core::types::CursorCol`
- `core::types::CursorPosition`
- `events/handlers/viewport::WindowSurfaceMetrics`
- `events/cursor/conceal::ConcealScreenCellView`
- `screenpos::command_row_from_dimensions()`
- `ExternalDemand.requested_target` unless you can justify a real semantic use

### Split

- `state::CursorLocation` into:
  - `WindowSurfaceSnapshot`
  - `CursorObservation.buffer_line`

### Move

- `CursorPositionSync` semantics into `ObservedCell`
- all shared position primitives into one shared module

### Keep, but rename for clarity

- `Point` → `RenderPoint`
- `latest_exact_cursor_position` → `latest_exact_cursor_cell`

---

## 5. Required invariants

These should exist as debug assertions and tests.

### Type invariants

- every stored `ScreenCell` is 1-based and positive
- every stored `BufferLine` is 1-based and positive
- every stored `ViewportBounds` has `max_row >= 1`, `max_col >= 1`
- every stored `WindowSurfaceSnapshot` has positive origin and positive dimensions

### Ownership invariants

- no module outside the shared position module defines a second integral screen-cell type
- no function besides the canonical surface reader parses `getwininfo`
- no function besides the canonical viewport object computes command row

### Behavioral invariants

- deferred cursor samples do not overwrite the exact-anchor cache
- deferred cursor samples still drive immediate motion target selection
- `retarget_epoch` changes iff the retarget key changes
- conceal cache keys are derived from the passed `WindowSurfaceSnapshot`, not from a second host read

---

## 6. Migration order

The clean order is:

1. Introduce the shared position module and canonical types.
2. Replace tuple aliases and `CursorPosition` conversions with `ScreenCell`.
3. Split `CursorLocation`.
4. Collapse `getwininfo` parsing into one surface reader.
5. Collapse cursor read selection into one `CursorObservation` result.
6. Move exactness into `ObservedCell`.
7. Unify runtime target mutation into one `CursorTarget`.
8. Remove `requested_target`.
9. Delete duplicate command-row logic.

That order minimizes churn while preserving behavior.

---

## Bottom line

The biggest problems are:

- `CursorLocation` mixing unrelated facts and allowing fake zero states
- multiple independent owners of the same window surface snapshot
- multiple isomorphic cursor-cell types with inconsistent invariants
- cursor exactness modeled as sidecar state instead of part of the cursor sample
- dead/ambiguous `requested_target`

The thing to preserve is the semantic separation between:

- newest observed cell
- latest exact anchor
- runtime target
- rendered pose

That is the right shape. The cleanup is to make each of those explicit, typed, and singly owned.

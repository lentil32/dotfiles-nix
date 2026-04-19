# Position Foundations Plan

This phase establishes the shared position vocabulary described in [position-spec.md](/Users/lentil32/.nixpkgs/nvim/rust/plans/position-spec.md). It should land before observation or runtime refactors so later phases can consume one canonical set of types.

## Goal

Create a shared `position` module for the `smear-cursor` crate and make it the only home for retained position primitives and invariants.

## Progress

- [x] Phase complete

## In Scope

- Introduce canonical shared types for:
  - `ScreenCell`
  - `BufferLine`
  - `ViewportBounds`
  - `SurfaceId`
  - `WindowSurfaceSnapshot`
  - `ObservedCell`
  - `CursorObservation`
  - `RenderPoint`
- Remove layer-local aliases and legacy types that represent the same facts with weaker invariants.
- Move validation for one-based screen coordinates and viewport bounds into the new shared types.

## Primary Modules

- `plugins/smear-cursor/src/core/types.rs`
- `plugins/smear-cursor/src/types.rs`
- `plugins/smear-cursor/src/events/cursor/mod.rs`
- `plugins/smear-cursor/src/state/cursor.rs`
- `plugins/smear-cursor/src/test_support/fixtures.rs`
- `plugins/smear-cursor/src/test_support/strategies.rs`

## Work Items

### 1. Introduce the shared module

- [x] Add `plugins/smear-cursor/src/position/` as the crate-level owner for position primitives.
- [x] Implement constructors and accessors that enforce the invariants from the spec instead of depending on scattered `row > 0` checks.
- [x] Use fallible constructors or `TryFrom` boundaries for raw host-derived values that may violate one-based invariants.
- [x] Keep the initial API small. If a helper is only needed once, inline it at the caller instead of creating new convenience methods.

### 2. Consolidate integral screen-cell types

- [x] Replace `core::types::{CursorRow, CursorCol, CursorPosition}` with `position::ScreenCell` and `position::ViewportBounds`.
- [x] Replace `events::cursor::{ScreenCell, ScreenPoint}` tuple aliases with the shared types.
- [x] Update test fixtures and strategies to generate the new canonical values directly.

### 3. Clarify render-space geometry

- [x] Rename `crate::types::Point` to `position::RenderPoint` and update call sites directly so discrete editor cells and continuous render geometry are clearly separated.

### 4. Eliminate sentinel-friendly constructors

- [x] Stop constructing `CursorLocation::new(0, 0, 0, 0)`-style placeholder values as part of the rewrite.
- [x] Where data can be absent, model that absence explicitly at the call site instead of carrying invalid numbers through the system.

## Rewrite Constraints

- [x] Do not keep compatibility aliases for `CursorRow`, `CursorCol`, `CursorPosition`, tuple screen cells, or tuple screen points.
- [x] Do not keep both `Point` and `RenderPoint` as retained public representations of the same fact.
- [x] Delete old type owners in the same phase that introduces their canonical replacements.

## Jane Street Gates

- [x] Represent one-based coordinates and positive viewport bounds with validated non-zero types rather than raw integers.
- [x] Use private fields plus validated constructors for invariant-carrying types instead of public raw fields.
- [x] Derive `Copy` only for genuinely small plain-data types that should move by value.
- [x] Keep shared type APIs borrowing-friendly. Do not force clones by API shape.
- [x] Avoid one-off helper methods that are referenced only once; prefer direct code at the call site unless a helper owns a reusable invariant.
- [x] Add module docs for `position` and item docs for shared types that become architectural vocabulary.
- [x] Split the shared module by owned fact if it starts growing into another kitchen-sink file.

## Out Of Scope

- Unifying host reads in `events/handlers/viewport.rs`
- Reworking observation storage in `core/state/observation`
- Changing runtime target selection or retargeting rules

Those belong to later phases and should build on the types introduced here.

## Done Criteria

- [x] No retained integral screen-cell type remains outside the shared `position` module.
- [x] `core::types::{CursorRow, CursorCol, CursorPosition}` and the tuple aliases in `events::cursor` are deleted.
- [x] New shared types enforce positivity and one-based indexing at construction boundaries.
- [x] Shared invariant-carrying types are constructor-owned and do not expose raw mutable fields.
- [x] Shared small position primitives use clear by-value semantics instead of reference-heavy or clone-heavy APIs.
- [x] Tests and fixtures create canonical position types directly.

## Targeted Tests

- [x] Leaf tests for `ScreenCell`, `BufferLine`, and `ViewportBounds` constructors and conversions.
- [x] Property tests for one-based invariants and round-trip conversions that are intentionally allowed by the spec.
- [x] Tests that malformed raw values are rejected at the constructor boundary instead of being normalized into fake states.
- [x] Smoke coverage proving migrated fixtures and strategies no longer depend on legacy cursor-position types.

## Phase Exit Notes

This phase should leave downstream code compiling against the new shared types without any compatibility layer. It is complete only when later phases can consume the rewritten type surface directly.

# Position Observation Plan

This phase converts the event-layer observation boundary from a mix of duplicated host reads and kitchen-sink structs into a small set of semantic facts. It assumes the shared types from [position-foundations.md](/Users/lentil32/.nixpkgs/nvim/rust/plans/position-foundations.md) already exist.

## Goal

Make observation collection produce exactly three position-related semantic facts:

- `WindowSurfaceSnapshot`
- `CursorObservation`
- `ViewportBounds`

The event layer may still use internal helpers, but only one retained representation of each fact should cross into reducer state.

## Progress

- [x] Phase complete

## Primary Modules

- `plugins/smear-cursor/src/events/handlers/viewport.rs`
- `plugins/smear-cursor/src/events/cursor/screenpos.rs`
- `plugins/smear-cursor/src/events/cursor/conceal.rs`
- `plugins/smear-cursor/src/events/handlers/observation/base.rs`
- `plugins/smear-cursor/src/core/state/observation/snapshot.rs`
- `plugins/smear-cursor/src/events/runtime/editor_viewport.rs`

## Work Items

### 1. Collapse surface reads into one owner

- [x] Replace the duplicated `getwininfo` parsing in `events/handlers/viewport.rs` and `events/cursor/conceal.rs` with one canonical surface reader.
- [x] Make that reader responsible for the retained `WindowSurfaceSnapshot` shape, including buffer/window identity, viewport translation fields, and window geometry.
- [x] Give the canonical reader an explicit typed error surface for malformed host data and parsing failures.
- [x] Pass the captured surface snapshot into conceal-related code instead of allowing conceal helpers to perform their own host read.

### 2. Replace `CursorLocation` with semantic observation facts

- [x] Remove `state::CursorLocation` from observation storage and ingress/runtime observation handoff.
- [x] Move surface facts into `WindowSurfaceSnapshot`.
- [x] Move buffer position context into `CursorObservation.buffer_line`.
- [x] Audit `derive_cursor_color_probe_witness()` and any ingress helpers that currently read `CursorLocation` fields directly.

### 3. Move cursor exactness into the observed cell

- [x] Replace the `CursorPositionSync` sidecar model with `ObservedCell`.
- [x] Refactor `events/cursor/screenpos.rs` so raw/conceal-adjusted selection stays local to the reader and does not escape as loosely-related fields.
- [x] Change `ObservationBasis` to store the normalized cursor sample directly.
- [x] Reduce `ObservationMotion` back to motion-only metadata, with scroll translation as the remaining position-adjacent concern.

### 4. Normalize observation construction

- [x] Update `events/handlers/observation/base.rs` to build `ObservationBasis` from the canonical surface, cursor, and viewport values.
- [x] Keep the fast-path and fallback policy explicit, but route them through one cursor-read contract and one surface-read contract.
- [x] Encode fallback policy as a named policy type or explicit branch structure, not as ad-hoc boolean plumbing.
- [x] Delete fake default observation states that depend on zero-filled `CursorLocation` instances.

## Rewrite Constraints

- [x] Do not keep `CursorLocation` as a compatibility carrier once `WindowSurfaceSnapshot` and `CursorObservation` exist.
- [x] Do not keep `CursorPositionSync` as a sidecar compatibility field after `ObservedCell` lands.
- [x] Do not keep a second host-read path for conceal correction or viewport-derived surface state.

## Jane Street Gates

- [x] Treat host-read parsing as a typed boundary: fallible reads return typed errors, semantic absence uses explicit enums, and production code does not panic.
- [x] Borrow the captured surface snapshot through the observation pipeline instead of cloning it by default.
- [x] Keep `ObservedCell` handling exhaustive. Exact, deferred, and unavailable cases should be explicit at the reader and reducer boundary.
- [x] Update `ObservationBasis::debug_assert_invariants()` and `ObservationSnapshot::debug_assert_invariants()` so they validate the new ownership model.
- [x] Keep malformed host data handling local to the observation boundary rather than leaking raw invalid values into reducer state.

## Out Of Scope

- Runtime target mutation rules
- `requested_target` removal
- Draw-layer viewport type cleanup

Those changes depend on the observation facts being stable first.

## Done Criteria

- [x] `CursorLocation` is no longer a retained owner of observation semantics.
- [x] Only one function parses `getwininfo` into retained surface data.
- [x] Conceal correction uses the caller-provided surface snapshot rather than performing an independent surface capture.
- [x] `ObservationBasis` owns surface, cursor, and viewport facts directly.
- [x] `ObservationMotion` no longer stores cursor exactness state.
- [x] Observation reads use typed error boundaries and explicit semantic absence instead of panic or fake defaults.

## Targeted Tests

- [x] Observation-base tests covering exact, deferred, and unavailable cursor reads.
- [x] Conceal-path tests proving deferred reads preserve the observed cell and do not re-read surface state through a second path.
- [x] Tests that malformed host payloads fail at the observation boundary without fabricating zero-valued surface or cursor state.
- [x] Reducer observation-completion tests that verify deferred cursor samples retain the prior exact anchor behavior once wired through the new `ObservedCell` model.

## Phase Exit Notes

Land the surface-reader collapse and the `ObservationBasis` rewrite together. This phase is complete only when the old observation carriers are gone instead of being preserved behind adapters.

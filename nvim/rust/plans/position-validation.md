# Position Validation And Rollout Plan

This file covers the checks needed to land the position refactor safely. It complements the implementation phases and avoids repeating the ownership rationale already documented in [position-spec.md](/Users/lentil32/.nixpkgs/nvim/rust/plans/position-spec.md).

## Goal

Make the refactor reviewable in small phases while keeping docs, tests, and invariants aligned with the new ownership model.

## Progress

- [x] Phase complete

## Jane Street Release Gates

- [x] Every touched production path satisfies the top-level quality gates in [plan.md](/Users/lentil32/.nixpkgs/nvim/rust/plan.md).
- [x] No touched production code uses `unwrap()`, `expect()`, or panic-driven control flow for recoverable position or host-read behavior.
- [x] Any new lint suppression uses `#[expect(...)]` with a precise justification instead of `#[allow(...)]`.
- [x] New shared modules and shared architectural types have current `//!` or `///` docs.
- [x] Invariant owners update their `debug_assert_invariants()` coverage in the same change as the ownership rewrite.

## Verification Matrix

### Leaf invariants

- [x] Shared position-type tests own positivity, one-based indexing, and allowed conversions.
- [x] Viewport-bound tests own the single command-row formula and `ViewportBounds` projection.
- [x] Surface-reader tests own `getwininfo` parsing and invalid-host-data rejection behavior.

### Observation behavior

- [x] Observation tests own exact, deferred, and unavailable cursor-sample behavior.
- [x] Conceal-related tests own the rule that surface facts come from the caller-provided snapshot, not an independent host capture.
- [x] Probe-witness tests own any cursor-color or text-context key changes caused by the new surface/cursor split.

### Reducer and runtime behavior

- [x] Reducer suites own exact-anchor retention, planning fallback selection, and removal of semantic shadow state.
- [x] Runtime suites own retarget-key semantics, target mutation behavior, and scroll-translation stability.
- [x] Snapshot tests only cover user-visible trace or diagnostic output that changes as a result of renamed fields or reshaped state.

## Verification Commands

- [x] Run `just fmt` after Rust code changes.
- [x] Run `cargo test -p nvimrs-smear-cursor`.
- [x] Run `just fix -p nvimrs-smear-cursor` before finalizing a large change in this crate.
- [x] If user-visible snapshot output changes, run `cargo insta test --test-runner nextest -p nvimrs-smear-cursor`, review pending snapshots, and accept the intended ones.
- [x] If changes extend into shared crates that require the full suite, ask the user before running `just test`. This refactor stayed within `nvimrs-smear-cursor`, so no `just test` run was needed.

## Documentation Updates

The implementation phases should update the following docs when their owned facts change:

- [x] [docs/state_ownership.md](/Users/lentil32/.nixpkgs/nvim/rust/docs/state_ownership.md)
  Update the observation, runtime, and shell-boundary ownership tables once the new types are real.
- [x] [docs/smear-cursor-testing-taxonomy.md](/Users/lentil32/.nixpkgs/nvim/rust/docs/smear-cursor-testing-taxonomy.md)
  Update only if the plan materially changes which layer owns a test matrix.

- [x] If the crate keeps doc-sync checks in `plugins/smear-cursor/src/doc_sync.rs`, add or update those checks in the same change that modifies the documented ownership surface.

## Landing Strategy

- [x] Land foundational type work first, with tests proving canonical construction and no new sentinel states.
- [x] Land observation refactors next, with focused reducer coverage for deferred cursor behavior.
- [x] Land runtime/planning cleanup after observation facts are stable.
- [x] Finish with doc updates and any required snapshot acceptance after the rewritten owners are in place.

Each phase should leave the crate in a buildable and testable state. Do not preserve old and new owners in parallel to make that happen; rewrite the phase boundary directly and remove the old owner in the same phase.

## Acceptance Checklist

- [x] There is one shared vocabulary for discrete screen cells, viewport bounds, surface snapshots, observed cells, and render points.
- [x] There is one retained owner for each semantic fact described in the spec.
- [x] There are no fake zero-valued cursor or surface placeholders in retained state.
- [x] `getwininfo` parsing and command-row math each have one implementation owner.
- [x] Conceal cache keys and wrapped-layout math project directly from `WindowSurfaceSnapshot` instead of a duplicate surface-view carrier.
- [x] Deferred observations still drive motion immediately without overwriting the exact-anchor cache.
- [x] Runtime target changes and `retarget_epoch` behavior are covered by explicit tests.
- [x] Settling refresh and promotion compare runtime target identity through `RuntimeTargetRetargetKey` instead of full `TrackedCursor` equality.
- [x] No backward-compatibility layer, migration shim, or dual-owner bridge remains in the landed code.
- [x] Lifecycle `jump_to_current_cursor()` reuses its first surface snapshot and cursor observation instead of a second tracked-cursor reconstruction helper or mixed live/stale fallback.
- [x] Cursor-color probe parsing only accepts the structured host-bridge payload; the legacy scalar compatibility branch is gone from runtime code and Lua regression harnesses.
- [x] `TrackedCursor` exposes the retained surface through one accessor (`surface()`) instead of a parallel `surface_snapshot()` alias.
- [x] Runtime config accepts frame timing only through `time_interval`; the `fps` boundary alias and its duplicate patch owner are gone from runtime code and Lua perf/test harnesses.
- [x] Host-bridge timer callbacks only flow through slot plus generation, and the cursor-color probe bridge requires its explicit fallback flag instead of retaining single-argument or varargs compatibility shims.
- [x] Cursor-color host-bridge calls no longer carry the removed highlight-cache `colorscheme_generation` argument once the Lua probe stopped using it.
- [x] Shell buffer-local cache invalidation reuses `invalidate_buffer_metadata()` as the single owner for dropping buffer perf policy entries; the duplicate second invalidation call is gone.
- [x] Scroll-shift viewport bounds project directly from `WindowSurfaceSnapshot` instead of mixing validated surface facts with live `window` geometry fallbacks.
- [x] Scroll-shift wrapped-distance clamping uses the validated surface viewport height instead of a second live `window.get_height()` read.
- [x] Cursor autocmd unchanged-fast-path derives `TrackedCursor` from the already-read `CursorObservation` instead of issuing a second `line('.')` host read beside the validated surface snapshot.
- [x] Touched production code follows constructor-owned invariants, explicit errors, exhaustive matches, and borrowing-first APIs.
- [x] Ownership docs and affected snapshots have been updated to match the landed code.

## Review Notes

- [x] Prefer PRs or commits that each answer one ownership question clearly.
- [x] When a lower layer gains full coverage, trim higher-level tests back to boundary smokes instead of preserving duplicate matrices.
- [x] If telemetry or trace output changes, review the generated snapshots as part of the same change instead of leaving `.snap.new` files for later cleanup.
- [x] Post-landing audit removed leftover legacy-equivalence naming and stale phase-roadmap comments so the landed code reflects the rewrite without backward-compat framing.
- [x] Post-landing audit dropped legacy-only rejection tests for removed option keys and removed host-bridge payload shapes instead of preserving explicit backward-compat coverage.
- [x] Post-landing audit removed the extra `SMEAR_CURSOR_LOG_FLUSH` alias spellings so runtime logging accepts only the canonical always-flush opt-in.
- [x] Post-landing audit trimmed Lua regression harnesses down to the current host-bridge contract by dropping legacy timer-bridge absence checks and the removed extra cursor-color probe argument.
- [x] Post-landing audit aligned `scripts/test_cursor_color_probe_extmarks.lua` with the single-argument cursor-color probe contract so the harness no longer relies on Lua ignoring the removed leading positional argument.
- [x] Post-landing audit collapsed conceal cache-key construction into `WindowSurfaceSnapshot`-based constructors so the surface projection lives in one place and the old argument-heavy helper constructors are gone.

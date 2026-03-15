# Smear Cursor Panic Patch Plan

## Goal

Remove or downgrade the remaining production panic points in `plugins/smear_cursor` so failures become explicit, deterministic lifecycle transitions, logged no-ops, or typed `Result` errors instead of `panic!`/`expect`/`unreachable!`.

This patch should preserve the project rules:

- state transitions define truth
- reducers stay pure
- effects stay deferred and isolated
- failure/retry remain modeled as normal lifecycle transitions

<!-- Surprising: exported plugin entrypoints and several shell-effect edges already use `catch_unwind`, so the current failure mode is mostly "runtime reset / plugin error" rather than "hard Neovim crash". -->
<!-- Surprising: `.tmp/nvim-oxi.nvim` was not present under this repo root during the audit, so this plan is based on the local `plugins/smear_cursor` crate only. -->

## Current Panic Inventory

Production-reachable or production-adjacent sites worth patching first:

- `plugins/smear_cursor/src/events.rs`
  - `EngineContext::take_state()` panics on re-entry with `engine state re-entered while already in use`.
- `plugins/smear_cursor/src/core/reducer/machine/planning.rs`
  - `build_planned_render()` uses `expect(...)` when building `InFlightProposal::{draw, clear, noop}`.
- `plugins/smear_cursor/src/events/handlers/render_apply.rs`
  - `ProposalExecution::Failure` currently hits `unreachable!(...)`.
- `plugins/smear_cursor/src/draw/palette.rs`
  - `refresh_highlight_palette_for_spec()` has an `unreachable!(...)` arm for `SkippedStale`.

Latent runtime panic points that do not show up in the explicit panic grep:

- `RefCell` borrow panics in singleton helpers:
  - `plugins/smear_cursor/src/events/event_loop.rs`
  - `plugins/smear_cursor/src/events/handlers/core_dispatch.rs`
  - `plugins/smear_cursor/src/events/logging.rs`
  - `plugins/smear_cursor/src/draw/palette.rs`
  - `plugins/smear_cursor/src/draw/mod.rs`

## Existing Containment

The next agent should keep these guards in place and only narrow the panic surface underneath them:

- exported Lua entrypoints are wrapped in `guard_plugin_call()` in `plugins/smear_cursor/src/lib.rs`
- scheduled callbacks are wrapped in `schedule_guarded()` in `plugins/smear_cursor/src/events/timers.rs`
- render planning is wrapped in `catch_unwind` in `plugins/smear_cursor/src/events/handlers/render_plan.rs`
- render apply is wrapped in `catch_unwind` in `plugins/smear_cursor/src/events/handlers/render_bridge.rs`
- some draw/palette state holders already reset internal caches after unwind

Do not remove those wrappers as part of this patch. Reduce the need for them.

## Patch Order

### Phase 1: Remove the low-blast-radius invariant panics

Start here. These changes are local, easy to verify, and reduce production panic count without forcing a broad API refactor.

1. Render-plan construction [done]

- Change `build_planned_render()` in `plugins/smear_cursor/src/core/reducer/machine/planning.rs` to return `Result<PlannedRender, E>` instead of panicking.
- Prefer reusing the existing proposal-shape error from `InFlightProposal::{draw, clear, noop}` if it is already expressive enough.
- If a wrapper error is needed, use a small `thiserror` enum such as `RenderPlanBuildError`.
- Update `execute_core_request_render_plan_effect()` to map `Err(...)` into the existing `RenderPlanFailed` path.
- Keep the `catch_unwind` there for truly unexpected bugs, but log the typed error separately so it is distinguishable from a panic.

2. Shell apply path [done]

- Replace the `unreachable!` for `ProposalExecution::Failure` in `plugins/smear_cursor/src/events/handlers/render_apply.rs`.
- Add a typed `ApplyRenderActionError` variant for this invalid shell entry, for example `FailureProposalReachedShell`.
- In `render_bridge`, map that error into the existing `ApplyReported::ApplyFailed` path rather than panicking.
- Keep the current outer `catch_unwind` as the last-resort guard.

3. Palette refresh path [done]

- Replace the `unreachable!` arm in `plugins/smear_cursor/src/draw/palette.rs` with an explicit stale-state fallback.
- Recommended behavior:
  - use `debug_assert!` to keep the invariant visible in debug/test builds
  - return `Ok(PaletteRefreshOutcome::SkippedStale)` or a similar non-panicking fallback in release
  - optionally log a warning once if that branch becomes reachable
- Do not silently mutate unrelated palette state in that fallback.

### Phase 2: Remove the engine-state re-entry panic

This is the highest-severity item, but it has a wider API impact than Phase 1.

1. Refactor the engine checkout path [done]

- Change `EngineContext::take_state()` in `plugins/smear_cursor/src/events.rs` to stop panicking on `EngineStateSlot::InUse`.
- Introduce an explicit error or outcome type, for example:
  - `EngineAccessError::Reentered`
  - or `enum EngineCheckout { Ready(EngineState), Reentered }`

2. Refactor `with_engine_state_access()` [done]

- `with_engine_state_access()` in `plugins/smear_cursor/src/events/runtime.rs` currently assumes checkout always succeeds and only handles panics after checkout.
- Rework it so re-entry becomes a normal failure path before the closure runs.
- Preferred direction:
  - add `try_with_engine_state_access<R>(...) -> Result<R, EngineAccessError>`
  - keep thin helpers that map this into existing plugin/runtime failure reporting where needed
- Avoid introducing a hidden fallback value for generic `R`; that will make failures invisible.

3. Model the failure explicitly [done]

- Re-entry should become one of:
  - a typed `Result` returned to the plugin entry boundary
  - a logged no-op plus an existing failure event
  - a runtime reset with an explicit warning and deterministic follow-up transition
- Do not replace the panic with silent early return.
- Add a brief comment explaining why re-entry is treated as a modeled lifecycle failure.

<!-- Surprising: `with_engine_state_access()` already resets runtime state after an unwind, but the initial checkout still panics before the reducer can model anything. -->

### Phase 3: Audit the singleton `RefCell` helpers

After the explicit panic macros are removed, there are still borrow-rule panics available through `RefCell`.

1. Audit helpers that use `borrow_mut()` directly [done]

- `plugins/smear_cursor/src/events/event_loop.rs`
- `plugins/smear_cursor/src/events/handlers/core_dispatch.rs`
- `plugins/smear_cursor/src/events/logging.rs`
- `plugins/smear_cursor/src/draw/palette.rs`
- `plugins/smear_cursor/src/draw/mod.rs`

2. Decide helper by helper [done]

- If re-entry is genuinely impossible and already enforced structurally, keep `borrow_mut()` and add a short comment saying why.
- If re-entry is plausible on scheduled edges or logging paths, switch to `try_borrow_mut()` and degrade gracefully.
- Logging should never be able to panic the plugin. If `LOG_FILE_HANDLE` is already borrowed, fall back to `api::err_writeln` or skip the file write.

3. Keep scope tight [done]

- This phase is allowed to stop after the helpers most likely to run during callback nesting are protected.
- Do not broaden into unrelated architectural cleanup.

## Test Plan

Add targeted regression tests as each phase lands.

1. Planning path

- Add a test proving an invalid proposal shape returns `RenderPlanFailed` or a typed error path instead of panicking.
- Prefer a descriptive module/test name under the existing render-plan or reducer tests.

2. Apply path

- Add a test proving `ProposalExecution::Failure` is converted into an apply failure event or typed apply error, not `unreachable!`.

3. Palette path

- Add a test forcing the stale palette branch and asserting the function returns a stable outcome without panic.

4. Engine re-entry

- Add a test that intentionally exercises nested engine-state access and verifies:
  - no panic escapes
  - runtime state is reset or failure is reported deterministically
  - subsequent accesses still work

5. RefCell helper hardening

- Add focused tests only where behavior changes.
- Follow the existing test style: descriptive names, one behavior per test where practical.

## Verification Commands

Run at minimum:

```bash
cargo test -p rs_smear_cursor --lib
```

If the patch changes signatures or adds error enums, also run:

```bash
cargo clippy -p rs_smear_cursor --all-targets -- -D warnings
```

Useful grep after patching:

```bash
rg -n "panic!|expect\\(|unwrap\\(|unreachable!" plugins/smear_cursor/src --glob '!**/tests.rs'
```

That grep does not catch `RefCell` borrow panics, so Phase 3 still needs a manual audit even if the grep is clean.

## Acceptance Criteria

Minimum acceptable handoff:

- no production `expect(...)` remains in `build_planned_render()`
- no production `unreachable!(...)` remains in the render-apply and palette paths identified above
- the plugin crate tests still pass

Full acceptance:

- engine-state re-entry no longer panics
- at least the highest-risk singleton `RefCell` helpers are protected or explicitly documented as structurally non-reentrant
- new tests cover every removed panic site

## Notes For The Next Agent

- Preserve deterministic state-machine semantics. If a failure happens after reducer state is committed, surface that as an explicit effect failure or apply failure, not a hidden rollback.
- Prefer `Result` and `thiserror` over ad hoc stringly-typed failures.
- Keep reducer purity intact. Error construction is fine in the effect layer; mutating shell/runtime state inside reducers is not.
- Do not touch test-only panic sites in this patch unless they block the refactor.

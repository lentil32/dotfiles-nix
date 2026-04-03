# smear-cursor follow-up plan

## Why this plan exists

The big performance rewrite is in good shape. The follow-up work should now be **narrow, evidence-driven, and easy to review**:

1. close the one remaining correctness ambiguity in cursor-color compatible reuse;
2. refresh the checked-in perf evidence on the exact final tree;
3. decide whether the new window-pool cap should really ship as the default;
4. add one runtime-level regression test so the adaptive policy is validated beyond pure helper tests.

The goal is not another broad rewrite. The goal is to make the current design **internally consistent, empirically justified, and easy to maintain**.

## Working rules

- Land this as **small patches**. Each patch should have a tight scope, an explicit acceptance bar, and targeted tests.
- Prefer **semantic clarity over cleverness**. If a name says “within line”, the implementation and cache key should also mean “within line”.
- Prefer **evidence-backed defaults**. If a new default is not clearly better, keep the safer default and document why.
- Keep the current architecture. Do **not** start a new planner rewrite in this round.
- Whenever diagnostics text changes, update the snapshot coverage.
- After Rust code changes, run `just fmt` and targeted crate tests. If snapshots change, use the `insta` flow for `nvimrs-smear-cursor`.

## Definition of done for this round

This follow-up round is done when all of the following are true:

- cursor-color compatible reuse semantics are unambiguous and fully tested;
- checked-in perf reports reflect the exact current branch, not an older intermediate tree;
- `DEFAULT_MAX_KEPT_WINDOWS` has an explicit, measured justification;
- at least one regression test proves that runtime pressure telemetry changes the resolved policy and/or diagnostics;
- `plan.md` can be deleted after the work lands because the code, tests, docs, and perf reports are self-explanatory.

---

## Patch 1 — Make cursor-color compatible reuse semantics exact and local

**Priority:** P0  
**Theme:** correctness + clarity  
**Expected size:** small
**Status:** done

### Problem

The code currently advertises `CursorColorReuseMode::CompatibleWithinLine` in `src/core/effect.rs`, and the motion cache key in `src/events/probe_cache/cursor_color.rs` is also line-scoped.

But the validator in `src/events/handlers/observation/cursor_color.rs` currently treats **any** position drift in the same buffer/mode/changedtick/colorscheme as compatible, including line changes.

That leaves three things out of sync:

- the enum name;
- the compatibility validator;
- the motion-cache key.

### Decision

Keep the existing product shape and make it truly **line-scoped**.

That means compatible cursor-color reuse should mean:

- same buffer;
- same changedtick;
- same mode;
- same colorscheme generation;
- same line;
- column drift allowed.

This matches the existing enum name and the existing motion-cache key, and it is the least surprising contract.

### Code changes

1. **Tighten validation in `src/events/handlers/observation/cursor_color.rs`.**
   - Update `validate_cursor_color_probe_witness()` so `ProbeReuse::Compatible` is returned only when both witnesses have a cursor position and `row` matches.
   - A row change must become `ProbeReuse::RefreshRequired`.

2. **Audit fallback-sample reuse in `collect_cursor_color_report()`.**
   - Ensure `payload.cursor_color_fallback_sample` is surfaced only for the same-line compatible case.
   - If validation says refresh is required, do not return a compatible fallback sample.

3. **Keep naming aligned.**
   - Preferred outcome: keep `CursorColorReuseMode::CompatibleWithinLine`.
   - Only rename if the intended product contract is actually broader than line scope. If that happens, rename all of the following together in the same patch:
     - `CursorColorReuseMode`
     - motion cache key comments/tests
     - diagnostics strings
     - observation tests
   - The preferred path is to fix behavior, not rename behavior.

4. **Audit `ProbePolicy::for_demand()` callsites.**
   - Confirm that full-mode probes can still use compatible same-line reuse when a fallback sample exists.
   - Confirm that crossing to a new line forces a refresh even in full-mode probes.

### Tests to add or update

In `src/events/handlers/observation/cursor_color.rs` tests:

- same row, different column => `Compatible`
- different row, same buffer/mode/changedtick/colorscheme => `RefreshRequired`
- exact mode + different column => `RefreshRequired`
- `cursor_color_probe_validation()` carries the current witness only for same-line compatible reuse

In `src/events/probe_cache/cursor_color.rs` tests:

- motion cache reuses within the same line
- motion cache misses across line boundaries
- exact cache entries remain distinct by full witness

In `src/core/effect.rs` tests:

- diagnostics still describe the expected probe-policy combinations after the semantics change

### Acceptance criteria

- enum name, validator, cache key, and tests all agree on the same contract;
- there is no test left that explicitly approves cross-line compatible reuse;
- a vertical cursor move cannot reuse a same-line compatible cursor-color sample.

### Risk / rollback notes

This may increase refresh frequency during vertical motion. That is acceptable unless perf data shows a material regression. If it does, measure it first; do not broaden semantics again without evidence.

---

## Patch 2 — Refresh the perf evidence on the final tree

**Priority:** P0  
**Theme:** measurement hygiene  
**Expected size:** small to medium
**Status:** done

### Problem

The architecture changed after some of the checked-in perf snapshots were captured. That means the repo currently mixes:

- reports that are still useful,
- reports that are directionally helpful,
- and reports that are no longer the final word on this exact tree.

The code is ahead of the evidence.

### Goal

Replace stale reports with measurements captured from the exact tree that will ship.

### Reports to refresh

Refresh these files in `plugins/smear-cursor/perf/`:

- `adaptive-buffer-policy-current.md`
- `planner-compile-current.md`
- `window-pool-cap-current.md`
- `particle-toggle-current.md`

### Capture rules

1. Run from a **clean working tree** if possible.
2. Make sure the report header includes:
   - capture time;
   - repo commit;
   - working tree state;
   - Neovim version;
   - command used.
3. If a report must be captured on a dirty tree, say why in the report or commit message.
4. Do not keep older stale “current” reports around after refreshing them.

### Expectations to verify

#### Adaptive policy snapshot

The important scenarios are the original complaint paths:

- `extmark_heavy`
- `conceal_heavy`
- `large_line_count`
- `long_running_repetition`

Expected outcome on the final tree:

- `auto` should degrade to `fast` / `fast_motion` for `extmark_heavy` and `conceal_heavy` once pressure is observed;
- `reason_bits` / reason summaries should mention the relevant pressure sources (`extmark`, `conceal_scan`, `conceal_raw`) when those pressures are driving the mode;
- fast-mode probe behavior should actually reduce expensive probe work compared with full mode.

Do **not** gate on absolute microsecond values across machines. Do gate on:

- realized mode / perf class;
- reason summaries;
- relative behavior of expensive probe counters.

#### Planner snapshot

Expected outcome:

- the bounded local-query path stays competitive on average baseline time;
- worst-case spikes do not regress materially compared with the reference path;
- telemetry for emitted compiled cells / candidate cells is explainable and stable enough to review.

This patch is **not** for redesigning the planner again. It is only for refreshing the evidence.

#### Window-pool cap snapshot

Expected outcome:

- the comparison isolates the cap change itself;
- cap hits remain visible in the report;
- the report is good enough to support a default-value decision in Patch 3.

#### Particle snapshot

Expected outcome:

- same deterministic trajectory in both cases;
- only `SMEAR_PARTICLES_ENABLED` changes between runs;
- the tax of particles is documented clearly enough to justify keeping them off by default.

### Docs work in the same patch

Update `plugins/smear-cursor/perf/README.md` so it matches the final harness shape and explicitly says:

- these are point-in-time local perf snapshots;
- they are for diffing behavior, not machine-independent golden numbers;
- which reports must be regenerated when planner / policy / particle / pool logic changes.

### Nice-to-have guardrail

Add either:

- a tiny helper script under `plugins/smear-cursor/scripts/` that extracts the key expectations from the perf reports; or
- a short documented checklist in `perf/README.md` that reviewers should verify after rerunning the reports.

Keep this lightweight. The purpose is review hygiene, not full perf CI.

### Acceptance criteria

- all four reports are refreshed on the final tree;
- the repo no longer contains stale “current” reports for older intermediate code;
- `perf/README.md` matches the final capture workflow.

---

## Patch 3 — Re-decide `DEFAULT_MAX_KEPT_WINDOWS`

**Priority:** P1  
**Theme:** default-value discipline  
**Expected size:** small

### Problem

`DEFAULT_MAX_KEPT_WINDOWS` in `src/config.rs` is now `64`, but the checked-in comparison currently shows zero cap hits while also showing several scenarios where `64` is slower than `384`.

That does **not** mean `64` is wrong forever. It does mean the current checked-in evidence does not yet justify treating `64` as obviously better.

### Goal

Choose the default based on the refreshed cap report from Patch 2, not on intuition.

### Decision rule

After rerunning the cap comparison on the exact final tree:

- **Keep `64`** only if it is neutral or clearly beneficial on the scenarios we care about, while maintaining zero cap hits.
- **Revert to `384`** if `64` is still slower in several scenarios and the report shows no compensating benefit.

If the answer is still ambiguous, choose the conservative option and revert to `384`.

### Code changes

Primary file:

- `src/config.rs`

Possible supporting doc updates:

- `perf/window-pool-cap-current.md`
- `perf/README.md`

### What not to change in this patch

- do not redesign the pool;
- do not change the adaptive retained-budget logic unless the perf report directly points there;
- do not couple this patch to unrelated planner or probe work.

### Acceptance criteria

- the shipped default has a checked-in measurement-based rationale;
- the code comment near `DEFAULT_MAX_KEPT_WINDOWS` matches that rationale;
- a reviewer can read the report and understand why the default is what it is.

---

## Patch 4 — Add runtime-level regression coverage for pressure-driven policy changes

**Priority:** P1  
**Theme:** correctness + observability  
**Expected size:** medium

### Problem

The repo now has strong **pure-policy** coverage for `BufferEventPolicy`, including hysteresis and reason ordering.

What is still missing is one test that proves the **live runtime path** does the right thing after telemetry is recorded. In other words, we want coverage that exercises more than the pure helper.

### Goal

Add at least one regression test that proves buffer-local telemetry can change the resolved policy and/or the emitted diagnostics for the active buffer.

### Preferred test shape

The best version is a runtime-oriented test that covers this chain:

1. buffer-local pressure is recorded;
2. the runtime reads telemetry for that buffer;
3. the current buffer policy is resolved;
4. diagnostics and/or effective mode reflect the pressure.

### Implementation options

#### Option A — true runtime-path test

If it is feasible without a lot of API scaffolding:

- seed telemetry via runtime/event-loop helpers;
- call `resolved_current_buffer_event_policy()` or a nearby runtime entry point;
- assert that the resulting policy is `auto_fast` for the pressured buffer;
- assert that the diagnostics summary contains the relevant reason.

Likely touchpoints:

- `src/events/runtime.rs`
- `src/events/event_loop.rs`
- `src/events/tests.rs`
- maybe `src/events/runtime/ingress_snapshot.rs`

#### Option B — extract a narrower runtime helper

If mocking `api::Buffer` or current-buffer state makes Option A too awkward:

- extract a small helper that consumes:
  - previous policy,
  - buffer metadata,
  - stored telemetry,
  - observed timestamp;
- keep the helper close to the runtime code path;
- test that helper using the real `BufferPerfTelemetry` objects and runtime-style inputs.

This is acceptable as long as the test still covers more than the current pure `BufferEventPolicy::from_*` units.

### Minimum test matrix

At least these cases should exist:

- extmark pressure flips auto mode to fast;
- conceal full-scan pressure flips auto mode to fast;
- conceal raw-screenpos pressure can keep fast motion after scan pressure decays;
- diagnostics still separate observed reasons from effective mode when a manual mode is forced.

The last item is especially useful because the diagnostics surface is now richer and easier to regress accidentally.

### Snapshot note

If the test updates diagnostics output, refresh the snapshot in:

- `src/events/snapshots/`

using the normal `insta` flow for `nvimrs-smear-cursor`.

### Acceptance criteria

- at least one regression test crosses the runtime boundary instead of only testing the pure policy helper;
- the test proves buffer-local pressure actually changes resolved behavior;
- diagnostics output remains stable and intentional.

---

## Patch 5 — Small cleanup and documentation pass

**Priority:** P2  
**Theme:** repo hygiene  
**Expected size:** small

### Goal

After the behavioral fixes and perf refreshes land, do one small cleanup pass so the codebase tells the same story everywhere.

### Checklist

- update comments in:
  - `src/core/effect.rs`
  - `src/events/handlers/observation/cursor_color.rs`
  - `src/events/probe_cache/cursor_color.rs`
  - `perf/README.md`
- remove or rewrite any outdated comments that still describe broader compatible reuse than the code allows;
- make sure diagnostics names remain accurate after Patch 1;
- make sure report headers and comments use the same terminology (`fast`, `fast_motion`, `exact`, `auto_fast`, etc.) consistently.

### Acceptance criteria

- no stale comment contradicts the new semantics or the refreshed reports;
- reviewers do not need oral history to understand why the code is shaped this way.

---

## Landing order

1. **Patch 1:** compatible-reuse semantics fix
2. **Patch 2:** perf-report refresh
3. **Patch 3:** window-cap default decision
4. **Patch 4:** runtime-level regression coverage
5. **Patch 5:** cleanup/docs pass

This order keeps correctness first, evidence second, default tuning third, and documentation last.

---

## Suggested commands during implementation

Use targeted commands; avoid a workspace-wide sweep until the local crate is healthy.

### After Patch 1 / Patch 4 / Patch 5

```bash
just fmt
cargo test -p nvimrs-smear-cursor
```

If diagnostics snapshots change:

```bash
cargo insta test --test-runner nextest -p nvimrs-smear-cursor
cargo insta pending-snapshots
cargo insta accept
```

### After Patch 2 / Patch 3

```bash
SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/adaptive-buffer-policy-current.md \
plugins/smear-cursor/scripts/compare_buffer_perf_modes.sh

SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/planner-compile-current.md \
plugins/smear-cursor/scripts/compare_planner_perf.sh

SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/window-pool-cap-current.md \
plugins/smear-cursor/scripts/compare_window_pool_cap_perf.sh

SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/particle-toggle-current.md \
plugins/smear-cursor/scripts/compare_particle_toggle_perf.sh
```

If Patch 3 changes only the default cap, rerun the cap report after that change so the checked-in evidence matches the final default.

---

## Explicit non-goals for this follow-up round

Do **not** expand this round into any of the following unless the refreshed perf data reveals a fresh blocker:

- another planner architecture rewrite;
- particle-system redesign or adaptive particle throttling;
- new user-facing config knobs for probe policy behavior;
- broad diagnostics redesign.

The current branch already has the right architecture. This round is about **consistency, evidence, and confidence**.

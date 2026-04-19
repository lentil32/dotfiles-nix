# Perf Snapshots

These files are point-in-time measurements captured from the local smear-cursor perf harnesses.
They are not cross-machine golden numbers. They exist so we can diff behavior on the exact tree
that shipped and see which perf class or planner path the runtime selected at capture time.

Measurement-only validation and allocation counters now live behind the
crate-local `perf-counters` cargo feature. The dedicated capture scripts that
need those counters opt into that feature automatically when they build the
release cdylib.

Perf harnesses now emit machine-readable `PERF_JSON` records alongside the
human-readable log lines. The shell entrypoints remain the supported interface,
but log decoding is centralized in the workspace `nvimrs-smear-perf-report`
tool so report generation is driven by one typed schema instead of ad hoc
`grep` / `sed` extraction in each script.

Regenerate the checked-in reports whenever the corresponding behavior changes:

- Adaptive policy or probe-path changes: `adaptive-buffer-policy-current.md`
- Planner compile or local-query changes: `planner-compile-current.md`
- Window-pool cap or retained-window policy changes: `window-pool-cap-current.md`
- Particle step-path changes: `particle-toggle-current.md`
- Degraded particle-buffer policy changes: `particle-degraded-buffer-current.md`
- Long-animation particle allocation changes: `long-animation-allocation-current.md`
- Validation counter instrumentation changes: `validation-counters-current.md`

If you refresh several checked-in reports in one patch, later captures may record
`Working tree: dirty` because earlier reports in the same patch are already updated.
Call that out in the commit message when it happens.

Reviewers should verify these expectations after rerunning the reports:

- Adaptive policy: the effective buffer mode should degrade to `auto_fast` for the pressure-heavy scenarios, the probe policy should shift toward `raw_syntax` when pressure is driving it, and the expensive probe counters should drop relative to `full`.
- Planner compile: the bounded local-query path should stay competitive on baseline time, worst-case spikes should remain explainable, and the emitted planner telemetry should match the realized path.
- Window cap: the local side should represent the shipped `64` default, cap hits should remain visible, and the report should still show why that smaller cap is acceptable relative to the measured peak demand.
- Particle toggle: both runs should share the same deterministic trajectory, only `SMEAR_PARTICLES_ENABLED` should differ, and the particle tax should stay explicit in the report.
- Degraded particle buffer policy: the `particles_on` workload should realize `full` vs `fast` as requested, the degraded `fast` path should stay competitive on baseline CPU, and both modes should keep probe fallback counters at zero.
- Long-animation particle allocations: the `long_running_repetition` workload should keep particles enabled, report both baseline CPU and allocation rates, and preserve the particle simulation or aggregation counts so future cache work has a stable comparison point.

Regenerate the adaptive buffer-policy snapshot with:

```bash
SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/adaptive-buffer-policy-current.md \
plugins/smear-cursor/scripts/compare_buffer_perf_modes.sh
```

Regenerate the planner compile snapshot with:

```bash
SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/planner-compile-current.md \
plugins/smear-cursor/scripts/compare_planner_perf.sh
```

Regenerate the particle toggle snapshot with:

```bash
SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/particle-toggle-current.md \
plugins/smear-cursor/scripts/compare_particle_toggle_perf.sh
```

This capture uses the dedicated `smear.step()` particle harness rather than the
window-switch scenario sweep, so both cases share the same deterministic
trajectory and only `SMEAR_PARTICLES_ENABLED` changes between them.

Regenerate the degraded particle-buffer snapshot with:

```bash
SMEAR_COMPARE_MODES=full,fast \
SMEAR_COMPARE_SCENARIOS=particles_on \
SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/particle-degraded-buffer-current.md \
plugins/smear-cursor/scripts/compare_buffer_perf_modes.sh
```

This capture keeps the current tree fixed and compares the `particles_on`
workload under `buffer_perf_mode=full` versus `buffer_perf_mode=fast` so the
degraded particle-path CPU envelope stays explicit.

Regenerate the long-animation allocation snapshot with:

```bash
SMEAR_LONG_ANIMATION_REPORT_FILE=plugins/smear-cursor/perf/long-animation-allocation-current.md \
plugins/smear-cursor/scripts/capture_long_animation_allocations.sh
```

This capture keeps the current tree fixed, enables particle effects for the
`long_running_repetition` workload, and records both baseline CPU plus
allocation deltas for the active animation window.

Regenerate the window-pool cap snapshot with:

```bash
SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/window-pool-cap-current.md \
plugins/smear-cursor/scripts/compare_window_pool_cap_perf.sh
```

This capture reuses the same window-switch code on both sides and only changes
`SMEAR_MAX_KEPT_WINDOWS` between the local shipped default (`64`) and the base
comparison value (`384`).

Regenerate the validation-counter baseline with:

```bash
SMEAR_VALIDATION_REPORT_FILE=plugins/smear-cursor/perf/validation-counters-current.md \
plugins/smear-cursor/scripts/capture_validation_counters.sh
```

This capture uses `validation_counters()` deltas between the harness warmup and
baseline phases so the saved rates represent the active animation window instead
of cumulative plugin lifetime totals.

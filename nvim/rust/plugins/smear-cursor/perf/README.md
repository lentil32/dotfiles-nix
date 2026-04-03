# Perf Snapshots

These files are point-in-time measurements captured from the local smear-cursor perf harnesses.
They are not cross-machine golden numbers. They exist so we can diff behavior on the exact tree
that shipped and see which perf class or planner path the runtime selected at capture time.

Regenerate the checked-in reports whenever the corresponding behavior changes:

- Adaptive policy or probe-path changes: `adaptive-buffer-policy-current.md`
- Planner compile or local-query changes: `planner-compile-current.md`
- Window-pool cap or retained-window policy changes: `window-pool-cap-current.md`
- Particle step-path changes: `particle-toggle-current.md`

If you refresh several checked-in reports in one patch, later captures may record
`Working tree: dirty` because earlier reports in the same patch are already updated.
Call that out in the commit message when it happens.

Reviewers should verify these expectations after rerunning the reports:

- Adaptive policy: the effective buffer mode should degrade to `auto_fast` for the pressure-heavy scenarios, the probe policy should shift toward `raw_syntax` when pressure is driving it, and the expensive probe counters should drop relative to `full`.
- Planner compile: the bounded local-query path should stay competitive on baseline time, worst-case spikes should remain explainable, and the emitted planner telemetry should match the realized path.
- Window cap: the local side should represent the shipped `64` default, cap hits should remain visible, and the report should still show why that smaller cap is acceptable relative to the measured peak demand.
- Particle toggle: both runs should share the same deterministic trajectory, only `SMEAR_PARTICLES_ENABLED` should differ, and the particle tax should stay explicit in the report.

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

Regenerate the window-pool cap snapshot with:

```bash
SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/window-pool-cap-current.md \
plugins/smear-cursor/scripts/compare_window_pool_cap_perf.sh
```

This capture reuses the same window-switch code on both sides and only changes
`SMEAR_MAX_KEPT_WINDOWS` between the local shipped default (`64`) and the base
comparison value (`384`).

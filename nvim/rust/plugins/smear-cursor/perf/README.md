# Perf Snapshot

`current.md` is the only checked-in smear-cursor perf snapshot.

These numbers are local point-in-time measurements. They are not cross-machine
golden thresholds. The snapshot exists to keep review evidence compact while
still showing the runtime paths that matter for smear-cursor performance.

Refresh it with:

```bash
plugins/smear-cursor/scripts/capture_perf_snapshot.sh
```

The command writes `plugins/smear-cursor/perf/current.md` by default. To compare
against a different base ref:

```bash
plugins/smear-cursor/scripts/capture_perf_snapshot.sh HEAD~1
```

The canonical snapshot intentionally has four sections:

- Adaptive buffer policy: compares `auto`, `full`, and `fast` on the current
  tree, including `particles_on`, so degraded particle-buffer behavior is part
  of the main buffer-policy matrix.
- Planner compile: compares the reference planner path against the bounded
  local-query path on the planner-heavy workload.
- Particle toggle: drives the deterministic direct `smear.step()` trajectory
  with particles off and on, so the particle tax stays isolated from
  window-switch noise.
- Window pool cap: compares the shipped `64` retained-window default with the
  historical `384` comparison value on the same scenarios.

The lower-level scripts under `plugins/smear-cursor/scripts/` are still useful
as focused probes. Do not add another checked-in perf markdown file for routine
work; extend `capture_perf_snapshot.sh` only when a new perf dimension is
important enough to be reviewed every time.

Allocation and validation-counter captures remain ad-hoc probes:

```bash
plugins/smear-cursor/scripts/capture_long_animation_allocations.sh
plugins/smear-cursor/scripts/capture_validation_counters.sh
```

Run those only when changing allocation behavior, validation instrumentation, or
the counters themselves.

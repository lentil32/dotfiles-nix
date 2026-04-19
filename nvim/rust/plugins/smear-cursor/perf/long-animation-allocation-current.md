# Long Animation Allocation Snapshot

- Captured (UTC): `2026-04-10T10:28:56Z`
- Repo commit: `5f01957cb6fa64a93ba15e5f98c6bb8a46c98c33`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.7`
- Command: `SMEAR_LONG_ANIMATION_REPORT_FILE=plugins/smear-cursor/perf/long-animation-allocation-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/capture_long_animation_allocations.sh`
- Config: repeats=`2`, scenario=`long_running_repetition`, buffer_perf_mode=`full`, particles_enabled=`true`, warmup=`300`, baseline=`1200`, windows=`8`, drain_every=`1`

These rates use the delta between `PERF_VALIDATION phase=post_warmup` and
`PERF_VALIDATION phase=post_baseline` so the allocation counts represent the
active long-animation window rather than cumulative plugin lifetime totals.

## Raw Results

```text
scenario                 run  baseline_elapsed_ms  baseline_avg_us  perf_class  probe_policy  particle_simulation_steps  particle_aggregation_calls  planning_preview_invocations  planning_preview_copied_particles  particle_overlay_refreshes  allocation_ops  allocation_ops_per_s  allocation_bytes  allocation_bytes_per_s
long_running_repetition  1    1879.785             1566.488         full        exact         225                        213                         1200                          5295                               0                           955445          508273.553            306638757         163124376.990
long_running_repetition  2    1877.640             1564.700         full        exact         225                        185                         1200                          2779                               0                           942038          501713.854            302368851         161036647.600
```

## Summary

```text
scenario                 avg_baseline_ms  avg_baseline_us  perf_class  probe_policy  avg_particle_simulation_steps  avg_particle_aggregation_calls  avg_planning_preview_invocations  avg_planning_preview_copied_particles  avg_particle_overlay_refreshes  avg_allocation_ops  avg_allocation_ops_per_s  avg_allocation_bytes  avg_allocation_bytes_per_s
long_running_repetition  1878.713         1565.594         full        exact         225.0                          199.0                           1200.0                            4037.0                                 0.0                             948741.5            504993.704                304503804.0           162080512.295
```

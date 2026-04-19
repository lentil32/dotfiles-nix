# Long Animation Allocation Snapshot

- Captured (UTC): `2026-04-07T08:59:26Z`
- Repo commit: `e90fd203838bb5adf4e64a6a8dcf830fe4929503`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.7`
- Command: `SMEAR_LONG_ANIMATION_REPORT_FILE=perf/long-animation-allocation-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/capture_long_animation_allocations.sh`
- Config: repeats=`2`, scenario=`long_running_repetition`, buffer_perf_mode=`full`, particles_enabled=`true`, warmup=`300`, baseline=`1200`, windows=`8`, drain_every=`1`

These rates use the delta between `PERF_VALIDATION phase=post_warmup` and
`PERF_VALIDATION phase=post_baseline` so the allocation counts represent the
active long-animation window rather than cumulative plugin lifetime totals.

## Raw Results

```text
scenario                 run  baseline_elapsed_ms  baseline_avg_us  perf_class  probe_policy  particle_simulation_steps  particle_aggregation_calls  planning_preview_invocations  planning_preview_copied_particles  particle_overlay_refreshes  allocation_ops  allocation_ops_per_s  allocation_bytes  allocation_bytes_per_s
long_running_repetition  1    2263.125             1885.938         full        exact         272                        250                         1200                          7953                               0                           1005005         444078.431            389875760         172273188.622
long_running_repetition  2    2295.615             1913.013         full        exact         275                        256                         1200                          7471                               0                           1018402         443629.267            394995538         172065236.549
```

## Summary

```text
scenario                 avg_baseline_ms  avg_baseline_us  perf_class  probe_policy  avg_particle_simulation_steps  avg_particle_aggregation_calls  avg_planning_preview_invocations  avg_planning_preview_copied_particles  avg_particle_overlay_refreshes  avg_allocation_ops  avg_allocation_ops_per_s  avg_allocation_bytes  avg_allocation_bytes_per_s
long_running_repetition  2279.370         1899.476         full        exact         273.5                          253.0                           1200.0                            7712.0                                 0.0                             1011703.5           443853.849                392435649.0           172169212.586
```

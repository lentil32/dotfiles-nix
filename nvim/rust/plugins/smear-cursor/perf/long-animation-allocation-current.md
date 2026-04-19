# Long Animation Allocation Snapshot

- Captured (UTC): `2026-04-16T07:47:41Z`
- Repo commit: `d425ed25eb26f12bdbdf84a5839b87f8f81579db`
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
long_running_repetition  1    1721.005             1434.171         full        exact         206                        170                         1200                          2924                               0                           1274241         740405.170            325244037         188984946.005
long_running_repetition  2    1715.260             1429.384         full        exact         205                        149                         1200                          2884                               0                           1248846         728079.708            319052503         186008245.397
```

## Summary

```text
scenario                 avg_baseline_ms  avg_baseline_us  perf_class  probe_policy  avg_particle_simulation_steps  avg_particle_aggregation_calls  avg_planning_preview_invocations  avg_planning_preview_copied_particles  avg_particle_overlay_refreshes  avg_allocation_ops  avg_allocation_ops_per_s  avg_allocation_bytes  avg_allocation_bytes_per_s
long_running_repetition  1718.133         1431.778         full        exact         205.5                          159.5                           1200.0                            2904.0                                 0.0                             1261543.5           734242.439                322148270.0           187496595.701
```

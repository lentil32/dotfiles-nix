# Long Animation Allocation Snapshot

- Captured (UTC): `2026-04-03T16:15:19Z`
- Repo commit: `1bb322afa5265b23af5331390ac15025fd958c20`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.6`
- Command: `SMEAR_LONG_ANIMATION_REPORT_FILE=plugins/smear-cursor/perf/long-animation-allocation-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/capture_long_animation_allocations.sh`
- Config: repeats=`2`, scenario=`long_running_repetition`, buffer_perf_mode=`full`, particles_enabled=`true`, warmup=`300`, baseline=`1200`, windows=`8`, drain_every=`1`

These rates use the delta between `PERF_VALIDATION phase=post_warmup` and
`PERF_VALIDATION phase=post_baseline` so the allocation counts represent the
active long-animation window rather than cumulative plugin lifetime totals.

## Raw Results

```text
scenario                 run  baseline_elapsed_ms  baseline_avg_us  perf_class  probe_policy  particle_simulation_steps  particle_aggregation_calls  particle_overlay_refreshes  allocation_ops  allocation_ops_per_s  allocation_bytes  allocation_bytes_per_s
long_running_repetition  1    1916.605             1597.171         full        exact         230                        1023                        0                           938397          489614.188            327331568         170787182.544
long_running_repetition  2    1968.754             1640.628         full        exact         236                        1198                        0                           954925          485040.284            332937681         169110859.457
```

## Summary

```text
scenario                 avg_baseline_ms  avg_baseline_us  perf_class  probe_policy  avg_particle_simulation_steps  avg_particle_aggregation_calls  avg_particle_overlay_refreshes  avg_allocation_ops  avg_allocation_ops_per_s  avg_allocation_bytes  avg_allocation_bytes_per_s
long_running_repetition  1942.679         1618.899         full        exact         233.0                          1110.5                          0.0                             946661.0            487327.236                330134624.5           169949021.000
```

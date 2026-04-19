# Particle Toggle Perf Snapshot

- Captured (UTC): 2026-04-10T10:27:58Z
- Repo commit: `5f01957cb6fa64a93ba15e5f98c6bb8a46c98c33`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.7`
- Base ref: `98a75d3e8829e578d6dff2d5b062811026c0e5f0`
- Command: `SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/particle-toggle-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/compare_particle_toggle_perf.sh 98a75d3e8829e578d6dff2d5b062811026c0e5f0`
- Config: repeats=`3`, warmup=`600`, benchmark=`2400`, retarget_interval=`24`, time_interval_ms=`8.333333333333334`, particle_max_num=`100`

This benchmark drives the same deterministic `smear.step()` trajectory for both cases and only flips `SMEAR_PARTICLES_ENABLED`.

## Raw Results

```text
side   case           run  avg_us  avg_particles  max_particles  final_particles
local  particles_off  1    6.320   0.000          0              0
local  particles_off  2    6.263   0.000          0              0
local  particles_off  3    6.418   0.000          0              0
local  particles_on   1    17.941  13.100         33             7
local  particles_on   2    18.480  13.100         33             7
local  particles_on   3    17.880  13.100         33             7
base   particles_off  1    6.244   0.000          0              0
base   particles_off  2    6.390   0.000          0              0
base   particles_off  3    6.252   0.000          0              0
base   particles_on   1    18.162  13.100         33             7
base   particles_on   2    18.077  13.100         33             7
base   particles_on   3    18.212  13.100         33             7
```

## Summary

```text
side   case           avg_step_us  avg_particles  max_particles  avg_final_particles
local  particles_off  6.334        0.000          0              0.00
local  particles_on   18.100       13.100         33             7.00
base   particles_off  6.295        0.000          0              0.00
base   particles_on   18.150       13.100         33             7.00
```

## Worst-Case Repeats

```text
side   case           worst_step_us  worst_avg_particles  max_particles  worst_final_particles
local  particles_off  6.418          0.000                0              0
local  particles_on   18.480         13.100               33             7
base   particles_off  6.390          0.000                0              0
base   particles_on   18.212         13.100               33             7
```

## Particle Isolation (same side)

```text
side   particles_off_avg_step_us  particles_on_avg_step_us  particle_tax_pct  particles_on_avg_particles  particles_on_max_particles
local  6.334                      18.100                    +185.78%          13.100                      33
base   6.295                      18.150                    +188.31%          13.100                      33
```

## Delta (local vs base)

```text
case           local_avg_step_us  base_avg_step_us  delta_pct  local_avg_particles  base_avg_particles
particles_off  6.334              6.295             +0.61%     0.000                0.000
particles_on   18.100             18.150            -0.28%     13.100               13.100
```

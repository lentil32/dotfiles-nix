# Particle Toggle Perf Snapshot

- Captured (UTC): 2026-03-27T05:07:30Z
- Repo commit: `91d00093a179d32eaf86ad534ecc117e74c2fb23`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.6`
- Base ref: `HEAD`
- Command: `SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/particle-toggle-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/compare_particle_toggle_perf.sh HEAD`
- Config: repeats=`3`, warmup=`600`, benchmark=`2400`, retarget_interval=`24`, time_interval_ms=`8.333333333333334`, particle_max_num=`100`

This benchmark drives the same deterministic `smear.step()` trajectory for both cases and only flips `SMEAR_PARTICLES_ENABLED`.

## Raw Results

```text
side   case           run  avg_us  avg_particles  max_particles  final_particles
local  particles_off  1    6.207   0.000          0              0
local  particles_off  2    6.420   0.000          0              0
local  particles_off  3    6.201   0.000          0              0
local  particles_on   1    17.503  13.100         33             7
local  particles_on   2    17.493  13.100         33             7
local  particles_on   3    17.650  13.100         33             7
base   particles_off  1    6.131   0.000          0              0
base   particles_off  2    6.287   0.000          0              0
base   particles_off  3    6.311   0.000          0              0
base   particles_on   1    18.044  13.100         33             7
base   particles_on   2    17.863  13.100         33             7
base   particles_on   3    18.130  13.100         33             7
```

## Summary

```text
side   case           avg_step_us  avg_particles  max_particles  avg_final_particles
local  particles_off  6.276        0.000          0              0.00
local  particles_on   17.549       13.100         33             7.00
base   particles_off  6.243        0.000          0              0.00
base   particles_on   18.012       13.100         33             7.00
```

## Worst-Case Repeats

```text
side   case           worst_step_us  worst_avg_particles  max_particles  worst_final_particles
local  particles_off  6.420          0.000                0              0
local  particles_on   17.650         13.100               33             7
base   particles_off  6.311          0.000                0              0
base   particles_on   18.130         13.100               33             7
```

## Particle Isolation (same side)

```text
side   particles_off_avg_step_us  particles_on_avg_step_us  particle_tax_pct  particles_on_avg_particles  particles_on_max_particles
local  6.276                      17.549                    +179.62%          13.100                      33
base   6.243                      18.012                    +188.52%          13.100                      33
```

## Delta (local vs base)

```text
case           local_avg_step_us  base_avg_step_us  delta_pct  local_avg_particles  base_avg_particles
particles_off  6.276              6.243             +0.53%     0.000                0.000
particles_on   17.549             18.012            -2.57%     13.100               13.100
```

# Particle Toggle Perf Snapshot

- Captured (UTC): 2026-04-16T07:46:44Z
- Repo commit: `d425ed25eb26f12bdbdf84a5839b87f8f81579db`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.7`
- Base ref: `HEAD`
- Command: `SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/particle-toggle-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/compare_particle_toggle_perf.sh HEAD`
- Config: repeats=`3`, warmup=`600`, benchmark=`2400`, retarget_interval=`24`, time_interval_ms=`8.333333333333334`, particle_max_num=`100`

This benchmark drives the same deterministic `smear.step()` trajectory for both cases and only flips `SMEAR_PARTICLES_ENABLED`.

## Raw Results

```text
side   case           run  avg_us  avg_particles  max_particles  final_particles
local  particles_off  1    8.360   0.000          0              0
local  particles_off  2    6.313   0.000          0              0
local  particles_off  3    6.740   0.000          0              0
local  particles_on   1    18.426  13.100         33             7
local  particles_on   2    18.856  13.100         33             7
local  particles_on   3    18.426  13.100         33             7
base   particles_off  1    7.630   0.000          0              0
base   particles_off  2    6.491   0.000          0              0
base   particles_off  3    6.540   0.000          0              0
base   particles_on   1    18.683  13.100         33             7
base   particles_on   2    18.830  13.100         33             7
base   particles_on   3    18.255  13.100         33             7
```

## Summary

```text
side   case           avg_step_us  avg_particles  max_particles  avg_final_particles
local  particles_off  7.138        0.000          0              0.00
local  particles_on   18.569       13.100         33             7.00
base   particles_off  6.887        0.000          0              0.00
base   particles_on   18.589       13.100         33             7.00
```

## Worst-Case Repeats

```text
side   case           worst_step_us  worst_avg_particles  max_particles  worst_final_particles
local  particles_off  8.360          0.000                0              0
local  particles_on   18.856         13.100               33             7
base   particles_off  7.630          0.000                0              0
base   particles_on   18.830         13.100               33             7
```

## Particle Isolation (same side)

```text
side   particles_off_avg_step_us  particles_on_avg_step_us  particle_tax_pct  particles_on_avg_particles  particles_on_max_particles
local  7.138                      18.569                    +160.16%          13.100                      33
base   6.887                      18.589                    +169.92%          13.100                      33
```

## Delta (local vs base)

```text
case           local_avg_step_us  base_avg_step_us  delta_pct  local_avg_particles  base_avg_particles
particles_off  7.138              6.887             +3.64%     0.000                0.000
particles_on   18.569             18.589            -0.11%     13.100               13.100
```

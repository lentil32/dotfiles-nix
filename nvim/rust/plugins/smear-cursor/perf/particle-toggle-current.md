# Particle Toggle Perf Snapshot

- Captured (UTC): 2026-04-07T09:00:38Z
- Repo commit: `49f9635c9f3978c0752a2032008433ceebd1e01a`
- Working tree: `clean`
- Neovim: `NVIM v0.11.7`
- Base ref: `91d00093a179d32eaf86ad534ecc117e74c2fb23`
- Command: `SMEAR_COMPARE_REPORT_FILE=perf/particle-toggle-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/compare_particle_toggle_perf.sh 91d00093a179d32eaf86ad534ecc117e74c2fb23`
- Config: repeats=`3`, warmup=`600`, benchmark=`2400`, retarget_interval=`24`, time_interval_ms=`8.333333333333334`, particle_max_num=`100`

This benchmark drives the same deterministic `smear.step()` trajectory for both cases and only flips `SMEAR_PARTICLES_ENABLED`.

## Raw Results

```text
side   case           run  avg_us  avg_particles  max_particles  final_particles
local  particles_off  1    6.028   0.000          0              0
local  particles_off  2    6.103   0.000          0              0
local  particles_off  3    6.102   0.000          0              0
local  particles_on   1    17.042  13.100         33             7
local  particles_on   2    17.513  13.100         33             7
local  particles_on   3    17.134  13.100         33             7
base   particles_off  1    6.167   0.000          0              0
base   particles_off  2    6.029   0.000          0              0
base   particles_off  3    6.261   0.000          0              0
base   particles_on   1    17.429  13.100         33             7
base   particles_on   2    17.445  13.100         33             7
base   particles_on   3    17.582  13.100         33             7
```

## Summary

```text
side   case           avg_step_us  avg_particles  max_particles  avg_final_particles
local  particles_off  6.078        0.000          0              0.00
local  particles_on   17.230       13.100         33             7.00
base   particles_off  6.152        0.000          0              0.00
base   particles_on   17.485       13.100         33             7.00
```

## Worst-Case Repeats

```text
side   case           worst_step_us  worst_avg_particles  max_particles  worst_final_particles
local  particles_off  6.103          0.000                0              0
local  particles_on   17.513         13.100               33             7
base   particles_off  6.261          0.000                0              0
base   particles_on   17.582         13.100               33             7
```

## Particle Isolation (same side)

```text
side   particles_off_avg_step_us  particles_on_avg_step_us  particle_tax_pct  particles_on_avg_particles  particles_on_max_particles
local  6.078                      17.230                    +183.49%          13.100                      33
base   6.152                      17.485                    +184.21%          13.100                      33
```

## Delta (local vs base)

```text
case           local_avg_step_us  base_avg_step_us  delta_pct  local_avg_particles  base_avg_particles
particles_off  6.078              6.152             -1.21%     0.000                0.000
particles_on   17.230             17.485            -1.46%     13.100               13.100
```

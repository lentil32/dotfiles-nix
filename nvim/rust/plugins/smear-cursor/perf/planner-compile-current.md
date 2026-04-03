# Planner Compile Perf Snapshot

- Captured (UTC): 2026-03-27T05:02:21Z
- Repo commit: `91d00093a179d32eaf86ad534ecc117e74c2fb23`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.6`
- Command: `SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/planner-compile-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/compare_planner_perf.sh`
- Config: repeats=`2`, modes=`reference,local_query`, scenarios=`planner_heavy`, warmup=`300`, baseline=`600`, stress=`1200`, stress_rounds=`preset`, recovery=`600`, recovery_mode=`fixed`, settle_wait_ms=`250`, windows=`8`, drain_every=`1`, delay_event_to_smear=`0`

## Raw Results

```text
mode         scenario       run  baseline_us  recovery_ratio  stress_max_avg_us  stress_max_ratio  stress_tail_ratio  perf_class  line_count  planner_bucket_maps_scanned  planner_bucket_cells_scanned  planner_local_query_envelope_area_cells  planner_local_query_cells  planner_compiled_query_cells  planner_candidate_query_cells  planner_compiled_cells_emitted  planner_candidate_cells_built  planner_reference_compiles  planner_local_query_compiles  realized_path
reference    planner_heavy  1    1757.496     0.957           1757.496           1.000             0.959              full        12000       28790                        510289                        0                                        110058                     54957                         55101                          157420                          927212                         1777                        0                             reference
reference    planner_heavy  2    1711.350     0.935           1762.080           1.030             0.997              full        12000       29240                        519754                        0                                        116638                     58258                         58380                          155976                          908170                         1801                        0                             reference
local_query  planner_heavy  1    1767.701     0.907           1767.701           1.000             0.912              full        12000       33010                        610665                        2170514                                  120352                     60045                         60307                          224895                          905276                         0                           2504                          local_query
local_query  planner_heavy  2    1671.195     0.971           1710.323           1.023             1.006              full        12000       27464                        474916                        2156411                                  111114                     55516                         55598                          222979                          900928                         0                           2517                          local_query
```

## Summary

```text
mode         scenario       avg_baseline_us  avg_recovery_ratio  avg_stress_max_ratio  avg_stress_tail_ratio  perf_class  line_count  realized_path
reference    planner_heavy  1734.423         0.946               1.015                 0.978                  full        12000       reference
local_query  planner_heavy  1719.448         0.939               1.011                 0.959                  full        12000       local_query
```

## Worst-Case Spikes

```text
mode         scenario       worst_baseline_us  worst_recovery_ratio  worst_stress_max_avg_us  worst_stress_max_ratio  worst_stress_tail_ratio  realized_path
reference    planner_heavy  1757.496           0.957                 1762.080                 1.030                   0.997                    reference
local_query  planner_heavy  1767.701           0.971                 1767.701                 1.023                   1.006                    local_query
```

## Planner Telemetry

```text
mode         scenario       avg_planner_bucket_maps_scanned  max_planner_bucket_maps_scanned  avg_planner_bucket_cells_scanned  max_planner_bucket_cells_scanned  avg_planner_local_query_envelope_area_cells  max_planner_local_query_envelope_area_cells  avg_planner_local_query_cells  max_planner_local_query_cells  avg_planner_compiled_query_cells  max_planner_compiled_query_cells  avg_planner_candidate_query_cells  max_planner_candidate_query_cells  avg_planner_compiled_cells_emitted  max_planner_compiled_cells_emitted  avg_planner_candidate_cells_built  max_planner_candidate_cells_built  avg_planner_reference_compiles  avg_planner_local_query_compiles  realized_path
reference    planner_heavy  29015.00                         29240                            515021.50                         519754                            0.00                                         0                                            113348.00                      116638                         56607.50                          58258                             56740.50                           58380                              156698.00                           157420                              917691.00                          927212                             1789.00                         0.00                              reference
local_query  planner_heavy  30237.00                         33010                            542790.50                         610665                            2163462.50                                   2170514                                      115733.00                      120352                         57780.50                          60045                             57952.50                           60307                              223937.00                           224895                              903102.00                          905276                             0.00                            2510.50                           local_query
```

## Compile Deltas

```text
scenario       reference_avg_baseline_us  local_query_avg_baseline_us  local_query_vs_reference_pct  reference_avg_recovery_ratio  local_query_avg_recovery_ratio  reference_avg_stress_max_ratio  local_query_avg_stress_max_ratio  reference_avg_planner_bucket_maps_scanned  local_query_avg_planner_bucket_maps_scanned  reference_avg_planner_bucket_cells_scanned  local_query_avg_planner_bucket_cells_scanned  reference_avg_planner_compiled_cells_emitted  local_query_avg_planner_compiled_cells_emitted  reference_avg_planner_candidate_cells_built  local_query_avg_planner_candidate_cells_built  reference_avg_planner_reference_compiles  local_query_avg_planner_local_query_compiles
planner_heavy  1734.423                   1719.448                     -0.86%                        0.946                         0.939                           1.015                           1.011                             29015.00                                   30237.00                                     515021.50                                   542790.50                                     156698.00                                     223937.00                                       917691.00                                    903102.00                                      1789.00                                   2510.50
```

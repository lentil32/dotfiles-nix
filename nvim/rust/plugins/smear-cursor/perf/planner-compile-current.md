# Planner Compile Perf Snapshot

- Captured (UTC): 2026-04-07T09:02:12Z
- Repo commit: `a648f7d5247b88a37083933940f3075e13346cc2`
- Working tree: `clean`
- Neovim: `NVIM v0.11.7`
- Command: `SMEAR_COMPARE_REPORT_FILE=perf/planner-compile-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/compare_planner_perf.sh`
- Config: repeats=`2`, modes=`reference,local_query`, scenarios=`planner_heavy`, warmup=`300`, baseline=`600`, stress=`1200`, stress_rounds=`preset`, recovery=`600`, recovery_mode=`fixed`, settle_wait_ms=`250`, windows=`8`, drain_every=`1`, delay_event_to_smear=`0`

## Raw Results

```text
mode         scenario       run  baseline_us  recovery_ratio  stress_max_avg_us  stress_max_ratio  stress_tail_ratio  perf_class  line_count  planner_bucket_maps_scanned  planner_bucket_cells_scanned  planner_local_query_envelope_area_cells  planner_local_query_cells  planner_compiled_query_cells  planner_candidate_query_cells  planner_compiled_cells_emitted  planner_candidate_cells_built  planner_reference_compiles  planner_local_query_compiles  realized_path
reference    planner_heavy  1    1719.792     1.010           1750.581           1.018             1.018              full        12000       31666                        574658                        0                                        119084                     59421                         59663                          155387                          797362                         1797                        0                             reference
reference    planner_heavy  2    1721.616     0.948           1747.820           1.015             1.015              full        12000       25770                        438295                        0                                        103314                     51611                         51703                          154295                          789360                         1800                        0                             reference
local_query  planner_heavy  1    1543.891     1.134           1788.921           1.159             1.159              full        12000       31320                        556105                        1922357                                  115676                     57721                         57955                          239439                          799784                         0                           2734                          local_query
local_query  planner_heavy  2    1680.280     0.955           1733.637           1.032             1.025              full        12000       27744                        479102                        1892207                                  110439                     55141                         55298                          235509                          790532                         0                           2722                          local_query
```

## Summary

```text
mode         scenario       avg_baseline_us  avg_recovery_ratio  avg_stress_max_ratio  avg_stress_tail_ratio  perf_class  line_count  realized_path
reference    planner_heavy  1720.704         0.979               1.016                 1.016                  full        12000       reference
local_query  planner_heavy  1612.086         1.044               1.095                 1.092                  full        12000       local_query
```

## Worst-Case Spikes

```text
mode         scenario       worst_baseline_us  worst_recovery_ratio  worst_stress_max_avg_us  worst_stress_max_ratio  worst_stress_tail_ratio  realized_path
reference    planner_heavy  1721.616           1.010                 1750.581                 1.018                   1.018                    reference
local_query  planner_heavy  1680.280           1.134                 1788.921                 1.159                   1.159                    local_query
```

## Planner Telemetry

```text
mode         scenario       avg_planner_bucket_maps_scanned  max_planner_bucket_maps_scanned  avg_planner_bucket_cells_scanned  max_planner_bucket_cells_scanned  avg_planner_local_query_envelope_area_cells  max_planner_local_query_envelope_area_cells  avg_planner_local_query_cells  max_planner_local_query_cells  avg_planner_compiled_query_cells  max_planner_compiled_query_cells  avg_planner_candidate_query_cells  max_planner_candidate_query_cells  avg_planner_compiled_cells_emitted  max_planner_compiled_cells_emitted  avg_planner_candidate_cells_built  max_planner_candidate_cells_built  avg_planner_reference_compiles  avg_planner_local_query_compiles  realized_path
reference    planner_heavy  28718.00                         31666                            506476.50                         574658                            0.00                                         0                                            111199.00                      119084                         55516.00                          59421                             55683.00                           59663                              154841.00                           155387                              793361.00                          797362                             1798.50                         0.00                              reference
local_query  planner_heavy  29532.00                         31320                            517603.50                         556105                            1907282.00                                   1922357                                      113057.50                      115676                         56431.00                          57721                             56626.50                           57955                              237474.00                           239439                              795158.00                          799784                             0.00                            2728.00                           local_query
```

## Compile Deltas

```text
scenario       reference_avg_baseline_us  local_query_avg_baseline_us  local_query_vs_reference_pct  reference_avg_recovery_ratio  local_query_avg_recovery_ratio  reference_avg_stress_max_ratio  local_query_avg_stress_max_ratio  reference_avg_planner_bucket_maps_scanned  local_query_avg_planner_bucket_maps_scanned  reference_avg_planner_bucket_cells_scanned  local_query_avg_planner_bucket_cells_scanned  reference_avg_planner_compiled_cells_emitted  local_query_avg_planner_compiled_cells_emitted  reference_avg_planner_candidate_cells_built  local_query_avg_planner_candidate_cells_built  reference_avg_planner_reference_compiles  local_query_avg_planner_local_query_compiles
planner_heavy  1720.704                   1612.086                     -6.31%                        0.979                         1.044                           1.016                           1.095                             28718.00                                   29532.00                                     506476.50                                   517603.50                                     154841.00                                     237474.00                                       793361.00                                    795158.00                                      1798.50                                   2728.00
```

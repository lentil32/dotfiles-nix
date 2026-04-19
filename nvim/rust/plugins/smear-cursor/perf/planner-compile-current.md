# Planner Compile Perf Snapshot

- Captured (UTC): 2026-04-16T07:41:31Z
- Repo commit: `d425ed25eb26f12bdbdf84a5839b87f8f81579db`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.7`
- Command: `SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/planner-compile-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/compare_planner_perf.sh`
- Config: repeats=`2`, modes=`reference,local_query`, scenarios=`planner_heavy`, warmup=`300`, baseline=`600`, stress=`1200`, stress_rounds=`preset`, recovery=`600`, recovery_mode=`fixed`, settle_wait_ms=`250`, windows=`8`, drain_every=`1`, delay_event_to_smear=`0`

## Raw Results

```text
mode         scenario       run  baseline_us  recovery_ratio  stress_max_avg_us  stress_max_ratio  stress_tail_ratio  perf_class  line_count  planner_bucket_maps_scanned  planner_bucket_cells_scanned  planner_local_query_envelope_area_cells  planner_local_query_cells  planner_compiled_query_cells  planner_candidate_query_cells  planner_compiled_cells_emitted  planner_candidate_cells_built  planner_reference_compiles  planner_local_query_compiles  realized_path
reference    planner_heavy  1    1648.591     1.050           1779.725           1.080             1.078              full        12000       25218                        434046                        0                                        104302                     52106                         52196                          157180                          775177                         1857                        0                             reference
reference    planner_heavy  2    1647.246     1.052           1841.987           1.118             1.118              full        12000       29300                        506995                        0                                        111632                     55710                         55922                          157898                          771706                         1875                        0                             reference
local_query  planner_heavy  1    1648.277     1.077           1803.477           1.094             1.094              full        12000       26322                        455570                        1849178                                  104523                     52205                         52318                          245633                          782821                         0                           2868                          local_query
local_query  planner_heavy  2    1801.281     0.955           1801.281           1.000             0.999              full        12000       28762                        498442                        1805821                                  112569                     56168                         56401                          238518                          769882                         0                           2825                          local_query
```

## Summary

```text
mode         scenario       avg_baseline_us  avg_recovery_ratio  avg_stress_max_ratio  avg_stress_tail_ratio  perf_class  line_count  realized_path
reference    planner_heavy  1647.918         1.051               1.099                 1.098                  full        12000       reference
local_query  planner_heavy  1724.779         1.016               1.047                 1.046                  full        12000       local_query
```

## Worst-Case Spikes

```text
mode         scenario       worst_baseline_us  worst_recovery_ratio  worst_stress_max_avg_us  worst_stress_max_ratio  worst_stress_tail_ratio  realized_path
reference    planner_heavy  1648.591           1.052                 1841.987                 1.118                   1.118                    reference
local_query  planner_heavy  1801.281           1.077                 1803.477                 1.094                   1.094                    local_query
```

## Planner Telemetry

```text
mode         scenario       avg_planner_bucket_maps_scanned  max_planner_bucket_maps_scanned  avg_planner_bucket_cells_scanned  max_planner_bucket_cells_scanned  avg_planner_local_query_envelope_area_cells  max_planner_local_query_envelope_area_cells  avg_planner_local_query_cells  max_planner_local_query_cells  avg_planner_compiled_query_cells  max_planner_compiled_query_cells  avg_planner_candidate_query_cells  max_planner_candidate_query_cells  avg_planner_compiled_cells_emitted  max_planner_compiled_cells_emitted  avg_planner_candidate_cells_built  max_planner_candidate_cells_built  avg_planner_reference_compiles  avg_planner_local_query_compiles  realized_path
reference    planner_heavy  27259.00                         29300                            470520.50                         506995                            0.00                                         0                                            107967.00                      111632                         53908.00                          55710                             54059.00                           55922                              157539.00                           157898                              773441.50                          775177                             1866.00                         0.00                              reference
local_query  planner_heavy  27542.00                         28762                            477006.00                         498442                            1827499.50                                   1849178                                      108546.00                      112569                         54186.50                          56168                             54359.50                           56401                              242075.50                           245633                              776351.50                          782821                             0.00                            2846.50                           local_query
```

## Compile Deltas

```text
scenario       reference_avg_baseline_us  local_query_avg_baseline_us  local_query_vs_reference_pct  reference_avg_recovery_ratio  local_query_avg_recovery_ratio  reference_avg_stress_max_ratio  local_query_avg_stress_max_ratio  reference_avg_planner_bucket_maps_scanned  local_query_avg_planner_bucket_maps_scanned  reference_avg_planner_bucket_cells_scanned  local_query_avg_planner_bucket_cells_scanned  reference_avg_planner_compiled_cells_emitted  local_query_avg_planner_compiled_cells_emitted  reference_avg_planner_candidate_cells_built  local_query_avg_planner_candidate_cells_built  reference_avg_planner_reference_compiles  local_query_avg_planner_local_query_compiles
planner_heavy  1647.918                   1724.779                     +4.66%                        1.051                         1.016                           1.099                           1.047                             27259.00                                   27542.00                                     470520.50                                   477006.00                                     157539.00                                     242075.50                                       773441.50                                    776351.50                                      1866.00                                   2846.50
```

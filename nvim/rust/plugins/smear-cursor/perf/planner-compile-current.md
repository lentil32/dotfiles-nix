# Planner Compile Perf Snapshot

- Captured (UTC): 2026-04-10T10:27:40Z
- Repo commit: `5f01957cb6fa64a93ba15e5f98c6bb8a46c98c33`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.7`
- Command: `SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/planner-compile-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/compare_planner_perf.sh`
- Config: repeats=`2`, modes=`reference,local_query`, scenarios=`planner_heavy`, warmup=`300`, baseline=`600`, stress=`1200`, stress_rounds=`preset`, recovery=`600`, recovery_mode=`fixed`, settle_wait_ms=`250`, windows=`8`, drain_every=`1`, delay_event_to_smear=`0`

## Raw Results

```text
mode         scenario       run  baseline_us  recovery_ratio  stress_max_avg_us  stress_max_ratio  stress_tail_ratio  perf_class  line_count  planner_bucket_maps_scanned  planner_bucket_cells_scanned  planner_local_query_envelope_area_cells  planner_local_query_cells  planner_compiled_query_cells  planner_candidate_query_cells  planner_compiled_cells_emitted  planner_candidate_cells_built  planner_reference_compiles  planner_local_query_compiles  realized_path
reference    planner_heavy  1    1352.005     1.086           1365.193           1.010             1.010              full        12000       29512                        541811                        0                                        103091                     51385                         51706                          124848                          746248                         1455                        0                             reference
reference    planner_heavy  2    1259.127     1.176           1364.145           1.083             1.083              full        12000       27018                        483829                        0                                        101583                     50636                         50947                          122584                          737341                         1445                        0                             reference
local_query  planner_heavy  1    1734.480     0.956           1776.583           1.024             1.024              full        12000       37794                        706770                        1837010                                  128486                     64051                         64435                          239511                          773147                         0                           2819                          local_query
local_query  planner_heavy  2    1447.196     1.161           1799.243           1.243             1.243              full        12000       32812                        598046                        1882964                                  121724                     60698                         61026                          241096                          788172                         0                           2780                          local_query
```

## Summary

```text
mode         scenario       avg_baseline_us  avg_recovery_ratio  avg_stress_max_ratio  avg_stress_tail_ratio  perf_class  line_count  realized_path
reference    planner_heavy  1305.566         1.131               1.046                 1.046                  full        12000       reference
local_query  planner_heavy  1590.838         1.058               1.134                 1.134                  full        12000       local_query
```

## Worst-Case Spikes

```text
mode         scenario       worst_baseline_us  worst_recovery_ratio  worst_stress_max_avg_us  worst_stress_max_ratio  worst_stress_tail_ratio  realized_path
reference    planner_heavy  1352.005           1.176                 1365.193                 1.083                   1.083                    reference
local_query  planner_heavy  1734.480           1.161                 1799.243                 1.243                   1.243                    local_query
```

## Planner Telemetry

```text
mode         scenario       avg_planner_bucket_maps_scanned  max_planner_bucket_maps_scanned  avg_planner_bucket_cells_scanned  max_planner_bucket_cells_scanned  avg_planner_local_query_envelope_area_cells  max_planner_local_query_envelope_area_cells  avg_planner_local_query_cells  max_planner_local_query_cells  avg_planner_compiled_query_cells  max_planner_compiled_query_cells  avg_planner_candidate_query_cells  max_planner_candidate_query_cells  avg_planner_compiled_cells_emitted  max_planner_compiled_cells_emitted  avg_planner_candidate_cells_built  max_planner_candidate_cells_built  avg_planner_reference_compiles  avg_planner_local_query_compiles  realized_path
reference    planner_heavy  28265.00                         29512                            512820.00                         541811                            0.00                                         0                                            102337.00                      103091                         51010.50                          51385                             51326.50                           51706                              123716.00                           124848                              741794.50                          746248                             1450.00                         0.00                              reference
local_query  planner_heavy  35303.00                         37794                            652408.00                         706770                            1859987.00                                   1882964                                      125105.00                      128486                         62374.50                          64051                             62730.50                           64435                              240303.50                           241096                              780659.50                          788172                             0.00                            2799.50                           local_query
```

## Compile Deltas

```text
scenario       reference_avg_baseline_us  local_query_avg_baseline_us  local_query_vs_reference_pct  reference_avg_recovery_ratio  local_query_avg_recovery_ratio  reference_avg_stress_max_ratio  local_query_avg_stress_max_ratio  reference_avg_planner_bucket_maps_scanned  local_query_avg_planner_bucket_maps_scanned  reference_avg_planner_bucket_cells_scanned  local_query_avg_planner_bucket_cells_scanned  reference_avg_planner_compiled_cells_emitted  local_query_avg_planner_compiled_cells_emitted  reference_avg_planner_candidate_cells_built  local_query_avg_planner_candidate_cells_built  reference_avg_planner_reference_compiles  local_query_avg_planner_local_query_compiles
planner_heavy  1305.566                   1590.838                     +21.85%                       1.131                         1.058                           1.046                           1.134                             28265.00                                   35303.00                                     512820.00                                   652408.00                                     123716.00                                     240303.50                                       741794.50                                    780659.50                                      1450.00                                   2799.50
```

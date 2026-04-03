# Window Switch Scenario Perf Snapshot

- Captured (UTC): 2026-03-27T05:07:10Z
- Repo commit: `91d00093a179d32eaf86ad534ecc117e74c2fb23`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.6`
- Base ref: `HEAD`
- Command: `SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/window-pool-cap-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/compare_window_pool_cap_perf.sh HEAD`
- Local overrides: `SMEAR_MAX_KEPT_WINDOWS=64`
- Base overrides: `SMEAR_MAX_KEPT_WINDOWS=384`
- Config: repeats=`2`, scenarios=`large_line_count,long_running_repetition,planner_heavy,extmark_heavy,conceal_heavy`, warmup=`300`, baseline=`600`, stress=`1200`, stress_rounds=`preset`, recovery=`600`, recovery_mode=`fixed`, settle_wait_ms=`250`, windows=`8`, drain_every=`1`, delay_event_to_smear=`0`

## Raw Results

```text
side   scenario                 run  baseline_us  recovery_ratio  stress_max_avg_us  stress_max_ratio  stress_tail_ratio  perf_class  line_count  extmark_fallback_calls  conceal_full_scan_calls  planner_bucket_maps_scanned  planner_bucket_cells_scanned  planner_local_query_envelope_area_cells  planner_local_query_cells  planner_compiled_query_cells  planner_candidate_query_cells  planner_compiled_cells_emitted  planner_candidate_cells_built  pool_total_windows  pool_cached_budget  max_kept_windows  pool_peak_requested_capacity  pool_capacity_cap_hits
local  large_line_count         1    1614.910     0.975           1650.286           1.022             1.022              fast        50000       0                       0                        14538                        125754                        1139003                                  39989                      19784                         20205                          31218                           202163                         18                  32                  64                18                            0
local  large_line_count         2    1607.808     0.994           1610.065           1.001             0.990              fast        50000       0                       0                        15252                        135947                        1287516                                  41143                      20340                         20803                          35169                           225200                         14                  32                  64                14                            0
local  long_running_repetition  1    1581.774     0.996           1616.061           1.022             1.022              full        12000       0                       0                        22300                        197116                        2527690                                  68357                      33869                         34488                          72365                           473607                         17                  32                  64                17                            0
local  long_running_repetition  2    1597.626     1.037           1624.812           1.017             1.012              full        12000       0                       0                        27516                        264219                        2409535                                  77028                      38132                         38896                          72344                           469233                         18                  32                  64                18                            0
local  planner_heavy            1    1622.907     1.020           1760.137           1.085             1.058              full        12000       0                       0                        30064                        542934                        2194202                                  114056                     56955                         57101                          228144                          931311                         11                  32                  64                11                            0
local  planner_heavy            2    1728.717     0.971           1728.717           1.000             0.994              full        12000       0                       0                        27110                        477310                        2181989                                  107860                     53880                         53980                          225241                          918306                         12                  32                  64                12                            0
local  extmark_heavy            1    1630.532     1.012           1634.823           1.003             1.000              fast        4000        3                       0                        17226                        161537                        1131179                                  48792                      24162                         24630                          34181                           209982                         15                  32                  64                15                            0
local  extmark_heavy            2    1600.183     1.017           1618.248           1.011             1.003              fast        4000        3                       0                        19648                        194986                        1174091                                  51610                      25529                         26081                          33691                           211342                         17                  32                  64                17                            0
local  conceal_heavy            1    1604.050     0.995           1604.050           1.000             0.990              fast        4000        0                       4                        16542                        157996                        1256892                                  43375                      21494                         21881                          34621                           226915                         19                  32                  64                19                            0
local  conceal_heavy            2    1558.727     0.994           1573.784           1.010             1.010              fast        4000        0                       4                        15592                        145035                        1255359                                  41532                      20586                         20946                          33102                           219673                         17                  32                  64                17                            0
base   large_line_count         1    1630.606     0.943           1630.606           1.000             0.969              fast        50000       0                       0                        16802                        157278                        1230455                                  45713                      22618                         23095                          33801                           221479                         16                  32                  384               16                            0
base   large_line_count         2    1598.324     0.971           1598.324           1.000             0.973              fast        50000       0                       0                        17858                        172980                        1270549                                  48444                      23990                         24454                          33041                           216323                         13                  32                  384               13                            0
base   long_running_repetition  1    1589.877     1.032           1621.855           1.020             1.011              full        12000       0                       0                        19458                        168169                        2582230                                  61412                      30460                         30952                          73056                           478627                         15                  32                  384               15                            0
base   long_running_repetition  2    1569.467     1.024           1698.806           1.082             1.012              full        12000       0                       0                        26338                        239691                        2391655                                  75205                      37246                         37959                          70376                           449933                         14                  32                  384               14                            0
base   planner_heavy            1    1488.091     1.120           1724.186           1.159             1.113              full        12000       0                       0                        28648                        529747                        2163926                                  110170                     54999                         55171                          224443                          909131                         11                  32                  384               11                            0
base   planner_heavy            2    1568.578     1.029           1757.012           1.120             1.106              full        12000       0                       0                        26214                        461348                        2143469                                  108815                     54370                         54445                          230081                          901151                         11                  32                  384               11                            0
base   extmark_heavy            1    1630.038     1.011           1641.551           1.007             1.007              fast        4000        3                       0                        15660                        132789                        1106053                                  44175                      21846                         22329                          33202                           206375                         17                  32                  384               17                            0
base   extmark_heavy            2    1663.212     1.007           1663.212           1.000             0.987              fast        4000        3                       0                        16292                        143689                        1065064                                  44819                      22214                         22605                          31809                           190792                         17                  32                  384               17                            0
base   conceal_heavy            1    1607.274     1.010           1638.328           1.019             1.015              fast        4000        0                       4                        13022                        110591                        1170727                                  37128                      18400                         18728                          33482                           211875                         13                  32                  384               13                            0
base   conceal_heavy            2    1575.143     1.031           1633.575           1.037             1.037              fast        4000        0                       4                        13556                        123551                        1214117                                  39072                      19374                         19698                          34135                           214110                         18                  32                  384               18                            0
```

## Summary

```text
side   scenario                 avg_baseline_us  avg_recovery_ratio  avg_stress_max_ratio  avg_stress_tail_ratio  avg_extmark_fallback_calls  avg_conceal_full_scan_calls  perf_class  line_count
local  large_line_count         1611.359         0.984               1.011                 1.006                  0.00                        0.00                         fast        50000
local  long_running_repetition  1589.700         1.016               1.019                 1.017                  0.00                        0.00                         full        12000
local  planner_heavy            1675.812         0.996               1.042                 1.026                  0.00                        0.00                         full        12000
local  extmark_heavy            1615.358         1.014               1.007                 1.002                  3.00                        0.00                         fast        4000
local  conceal_heavy            1581.389         0.994               1.005                 1.000                  0.00                        4.00                         fast        4000
base   large_line_count         1614.465         0.957               1.000                 0.971                  0.00                        0.00                         fast        50000
base   long_running_repetition  1579.672         1.028               1.051                 1.011                  0.00                        0.00                         full        12000
base   planner_heavy            1528.334         1.075               1.139                 1.110                  0.00                        0.00                         full        12000
base   extmark_heavy            1646.625         1.009               1.003                 0.997                  3.00                        0.00                         fast        4000
base   conceal_heavy            1591.208         1.020               1.028                 1.026                  0.00                        4.00                         fast        4000
```

## Worst-Case Spikes

```text
side   scenario                 worst_baseline_us  worst_recovery_ratio  worst_stress_max_avg_us  worst_stress_max_ratio  worst_stress_tail_ratio  perf_class  line_count
local  large_line_count         1614.910           0.994                 1650.286                 1.022                   1.022                    fast        50000
local  long_running_repetition  1597.626           1.037                 1624.812                 1.022                   1.022                    full        12000
local  planner_heavy            1728.717           1.020                 1760.137                 1.085                   1.058                    full        12000
local  extmark_heavy            1630.532           1.017                 1634.823                 1.011                   1.003                    fast        4000
local  conceal_heavy            1604.050           0.995                 1604.050                 1.010                   1.010                    fast        4000
base   large_line_count         1630.606           0.971                 1630.606                 1.000                   0.973                    fast        50000
base   long_running_repetition  1589.877           1.032                 1698.806                 1.082                   1.012                    full        12000
base   planner_heavy            1568.578           1.120                 1757.012                 1.159                   1.113                    full        12000
base   extmark_heavy            1663.212           1.011                 1663.212                 1.007                   1.007                    fast        4000
base   conceal_heavy            1607.274           1.031                 1638.328                 1.037                   1.037                    fast        4000
```

## Planner Telemetry

```text
side   scenario                 avg_planner_bucket_maps_scanned  max_planner_bucket_maps_scanned  avg_planner_bucket_cells_scanned  max_planner_bucket_cells_scanned  avg_planner_local_query_envelope_area_cells  max_planner_local_query_envelope_area_cells  avg_planner_local_query_cells  max_planner_local_query_cells  avg_planner_compiled_query_cells  max_planner_compiled_query_cells  avg_planner_candidate_query_cells  max_planner_candidate_query_cells  avg_planner_compiled_cells_emitted  max_planner_compiled_cells_emitted  avg_planner_candidate_cells_built  max_planner_candidate_cells_built
local  large_line_count         14895.00                         15252                            130850.50                         135947                            1213259.50                                   1287516                                      40566.00                       41143                          20062.00                          20340                             20504.00                           20803                              33193.50                            35169                               213681.50                          225200
local  long_running_repetition  24908.00                         27516                            230667.50                         264219                            2468612.50                                   2527690                                      72692.50                       77028                          36000.50                          38132                             36692.00                           38896                              72354.50                            72365                               471420.00                          473607
local  planner_heavy            28587.00                         30064                            510122.00                         542934                            2188095.50                                   2194202                                      110958.00                      114056                         55417.50                          56955                             55540.50                           57101                              226692.50                           228144                              924808.50                          931311
local  extmark_heavy            18437.00                         19648                            178261.50                         194986                            1152635.00                                   1174091                                      50201.00                       51610                          24845.50                          25529                             25355.50                           26081                              33936.00                            34181                               210662.00                          211342
local  conceal_heavy            16067.00                         16542                            151515.50                         157996                            1256125.50                                   1256892                                      42453.50                       43375                          21040.00                          21494                             21413.50                           21881                              33861.50                            34621                               223294.00                          226915
base   large_line_count         17330.00                         17858                            165129.00                         172980                            1250502.00                                   1270549                                      47078.50                       48444                          23304.00                          23990                             23774.50                           24454                              33421.00                            33801                               218901.00                          221479
base   long_running_repetition  22898.00                         26338                            203930.00                         239691                            2486942.50                                   2582230                                      68308.50                       75205                          33853.00                          37246                             34455.50                           37959                              71716.00                            73056                               464280.00                          478627
base   planner_heavy            27431.00                         28648                            495547.50                         529747                            2153697.50                                   2163926                                      109492.50                      110170                         54684.50                          54999                             54808.00                           55171                              227262.00                           230081                              905141.00                          909131
base   extmark_heavy            15976.00                         16292                            138239.00                         143689                            1085558.50                                   1106053                                      44497.00                       44819                          22030.00                          22214                             22467.00                           22605                              32505.50                            33202                               198583.50                          206375
base   conceal_heavy            13289.00                         13556                            117071.00                         123551                            1192422.00                                   1214117                                      38100.00                       39072                          18887.00                          19374                             19213.00                           19698                              33808.50                            34135                               212992.50                          214110
```

## Pool Retention vs max_kept_windows

```text
side   scenario                 avg_pool_total_windows  avg_pool_cached_budget  max_kept_windows  avg_pool_total_pct_of_max  avg_pool_cached_budget_pct_of_max
local  large_line_count         16.00                   32.00                   64                25.00                      50.00
local  long_running_repetition  17.50                   32.00                   64                27.34                      50.00
local  planner_heavy            11.50                   32.00                   64                17.97                      50.00
local  extmark_heavy            16.00                   32.00                   64                25.00                      50.00
local  conceal_heavy            18.00                   32.00                   64                28.12                      50.00
base   large_line_count         14.50                   32.00                   384               3.78                       8.33
base   long_running_repetition  14.50                   32.00                   384               3.78                       8.33
base   planner_heavy            11.00                   32.00                   384               2.86                       8.33
base   extmark_heavy            17.00                   32.00                   384               4.43                       8.33
base   conceal_heavy            15.50                   32.00                   384               4.04                       8.33
```

## Pool Peak Pressure vs max_kept_windows

```text
side   scenario                 avg_pool_peak_requested_capacity  max_pool_capacity_cap_hits  max_kept_windows  avg_pool_peak_requested_pct_of_max
local  large_line_count         16.00                             0                           64                25.00
local  long_running_repetition  17.50                             0                           64                27.34
local  planner_heavy            11.50                             0                           64                17.97
local  extmark_heavy            16.00                             0                           64                25.00
local  conceal_heavy            18.00                             0                           64                28.12
base   large_line_count         14.50                             0                           384               3.78
base   long_running_repetition  14.50                             0                           384               3.78
base   planner_heavy            11.00                             0                           384               2.86
base   extmark_heavy            17.00                             0                           384               4.43
base   conceal_heavy            15.50                             0                           384               4.04
```

## Delta (local vs base)

```text
scenario                 local_avg_baseline_us  base_avg_baseline_us  baseline_delta_pct  local_avg_recovery_ratio  base_avg_recovery_ratio  local_avg_stress_max_ratio  base_avg_stress_max_ratio  local_avg_stress_tail_ratio  base_avg_stress_tail_ratio
large_line_count         1611.359               1614.465              -0.19%              0.984                     0.957                    1.011                       1.000                      1.006                        0.971
long_running_repetition  1589.700               1579.672              +0.63%              1.016                     1.028                    1.019                       1.051                      1.017                        1.011
planner_heavy            1675.812               1528.334              +9.65%              0.996                     1.075                    1.042                       1.139                      1.026                        1.110
extmark_heavy            1615.358               1646.625              -1.90%              1.014                     1.009                    1.007                       1.003                      1.002                        0.997
conceal_heavy            1581.389               1591.208              -0.62%              0.994                     1.020                    1.005                       1.028                      1.000                        1.026
```

## Decision

Keep `DEFAULT_MAX_KEPT_WINDOWS=64`.

The refreshed comparison still shows zero cap hits in every scenario, and the
largest measured requested capacity is only `18` windows. That leaves more than
3x headroom under the `64` cap while keeping the shipped default much closer to
the observed demand than `384`.

The `planner_heavy` baseline is the only clearly worse local result in this
capture. The rest of the baseline deltas are neutral to favorable, and none of
the stress or recovery sections show evidence that `64` is starving the pool.
Revisit this default only if a future report starts hitting the cap or shows a
repeatable multi-scenario regression from the smaller ceiling.

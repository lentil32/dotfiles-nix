# Smear Cursor Perf Snapshot

- Captured (UTC): 2026-04-26T09:24:39Z
- Repo commit: `cb4d12f2c3e4960993178355c7adc19364fc8374`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.7`
- Base ref: `HEAD`
- Command: `SMEAR_PERF_REPORT_FILE=plugins/smear-cursor/perf/current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/capture_perf_snapshot.sh HEAD`
- Artifacts: `/tmp/smear_perf_snapshot.5wUoII`
- Config: repeats=`2`, particle_repeats=`2`, buffer_modes=`auto,full,fast`, buffer_scenarios=`large_line_count,long_running_repetition,extmark_heavy,conceal_heavy,particles_on`, planner_modes=`reference,local_query`, planner_scenarios=`planner_heavy`, cap_scenarios=`large_line_count,long_running_repetition,planner_heavy,extmark_heavy,conceal_heavy`

This is the only checked-in smear-cursor perf snapshot. The numbers are local point-in-time measurements, not cross-machine golden thresholds.

## Adaptive Buffer Policy

### Raw Results

```text
mode  scenario                 run  baseline_us        recovery_ratio      stress_max_avg_us  stress_max_ratio   stress_tail_ratio   realized_mode  perf_class  probe_policy       line_count  callback_ewma_ms  reason_bits  extmark_fallback_calls  conceal_full_scan_calls
auto  large_line_count         1    1841.143125        0.9732749819037922  1854.672291666667  1.00734824277535   0.9969140363091904  auto_fast      fast        deferred_extmarks  50000       0.1               1            0                       0
auto  large_line_count         2    1881.915903333333  0.9353686130264578  1881.915903333333  1.0                0.9516868683280216  auto_fast      fast        deferred_extmarks  50000       0                 1            0                       0
auto  long_running_repetition  1    1949.042638333333  0.8860866309609374  1949.042638333333  1.0                0.9064465733515598  auto_full      full        exact              12000       0.1               0            0                       0
auto  long_running_repetition  2    1843.789653333333  0.9266881674441764  1848.263923333333  1.002426670521722  0.9444234361831468  auto_full      full        exact              12000       0.1               0            0                       0
auto  extmark_heavy            1    1821.017291666667  0.9768318216820886  1834.689305833333  1.007507899144743  0.9772676856560144  auto_fast      fast        deferred_extmarks  4000        0.1               16           6301                    0
auto  extmark_heavy            2    1762.296528333333  1.006677957697503   1848.904166666667  1.049144759091843  1.031366741698276   auto_fast      fast        deferred_extmarks  4000        0.1               16           6301                    0
auto  conceal_heavy            1    1787.900348333333  1.013420303330869   1866.007291666667  1.043686407582024  1.018235037090886   auto_fast      fast        deferred_extmarks  4000        0.1               32           0                       6300
auto  conceal_heavy            2    1908.13236         0.7667349694057212  1908.13236         1.0                0.8364729901825749  auto_fast      fast        deferred_extmarks  4000        0                 32           0                       6300
auto  particles_on             1    1437.024443333333  1.160455257206214   1857.79882         1.292809477680585  1.279307329713178   auto_full      full        exact              4000        0                 0            0                       0
auto  particles_on             2    1617.73257         1.036687065032016   1648.277951666667  1.01888160146684   0.9937940396621509  auto_full      full        exact              4000        0.2               0            0                       0
full  large_line_count         1    1571.529166666667  0.9320861400917896  1599.7071525       1.017930297719578  0.9700050195271588  full_full      full        exact              50000       0                 1            0                       0
full  large_line_count         2    1570.682846666667  1.206523365949876   1789.878090833333  1.139554108349656  1.139554108349656   full_full      full        exact              50000       0.2               1            0                       0
full  long_running_repetition  1    1717.315485        0.926938294431478   1817.320555833333  1.058233371623813  0.9174315516056736  full_full      full        exact              12000       0.1               0            0                       0
full  long_running_repetition  2    1739.686736666667  0.9402686599777704  1829.7027775       1.051742672365146  0.9329485964102338  full_full      full        exact              12000       0.1               0            0                       0
full  extmark_heavy            1    1759.73493         0.9246570552153178  1829.607465        1.039706284059498  0.8640472327651454  full_full      full        exact              4000        0.1               16           6301                    0
full  extmark_heavy            2    1602.168056666667  1.094207934495561   1740.515971666667  1.086350438971948  1.052681933905406   full_full      full        exact              4000        0.1               16           6301                    0
full  conceal_heavy            1    1677.929515        0.9888293619214792  1719.1284375       1.02455342857474   1.02455342857474    full_full      full        exact              4000        0.1               32           0                       6300
full  conceal_heavy            2    1708.09243         1.019706193534269   1708.09243         1.0                0.9737432690727787  full_full      full        exact              4000        0.1               32           0                       6300
full  particles_on             1    1679.42125         1.032974239389512   1730.469305833333  1.030396218836301  1.008211658232461   full_full      full        exact              4000        0.1               0            0                       0
full  particles_on             2    1743.517015        0.9984972501114364  1743.517015        1.0                0.9813614475489744  full_full      full        exact              4000        0.2               0            0                       0
fast  large_line_count         1    1682.493125        1.022021553084603   1700.242638333333  1.010549530972576  1.010549530972576   fast_fast      fast        deferred_extmarks  50000       0.1               1            0                       0
fast  large_line_count         2    1778.096735        0.93856832635524    1778.096735        1.0                0.9536033833952232  fast_fast      fast        deferred_extmarks  50000       0.1               1            0                       0
fast  long_running_repetition  1    1741.375763333333  0.977739544129676   1746.584895833333  1.002991389112956  0.9151195754075891  fast_fast      fast        deferred_extmarks  12000       0.2               0            0                       0
fast  long_running_repetition  2    1719.802083333333  0.968641336394086   1719.802083333333  1.0                0.9449956896687484  fast_fast      fast        deferred_extmarks  12000       0.1               0            0                       0
fast  extmark_heavy            1    1749.781596666667  0.9976326407395312  1749.781596666667  1.0                0.9743665432767276  fast_fast      fast        deferred_extmarks  4000        0.1               16           6301                    0
fast  extmark_heavy            2    1677.030278333333  0.9770992755291804  1726.893506666667  1.029733051917756  1.016305360545135   fast_fast      fast        deferred_extmarks  4000        0                 16           6301                    0
fast  conceal_heavy            1    1742.798263333333  0.9129665158663351  1742.798263333333  1.0                0.8902850003605026  fast_fast      fast        deferred_extmarks  4000        0.1               32           0                       6300
fast  conceal_heavy            2    1506.917638333333  1.025649028420745   1628.958368333334  1.080986994176389  1.080986994176389   fast_fast      fast        deferred_extmarks  4000        0                 32           0                       6300
fast  particles_on             1    1562.940208333333  1.030765955138239   1562.940208333333  1.0                0.9816803836465808  fast_fast      fast        deferred_extmarks  4000        0                 0            0                       0
fast  particles_on             2    1501.059723333333  1.040481400610138   1559.016041666667  1.038610268087557  1.03512092768896    fast_fast      fast        deferred_extmarks  4000        0.1               0            0                       0
```

### Summary

```text
mode  scenario                 avg_baseline_us  avg_recovery_ratio  avg_stress_max_ratio  avg_stress_tail_ratio  realized_mode  perf_class  probe_policy       line_count  avg_callback_ewma_ms  reason_bits
auto  large_line_count         1861.530         0.954               1.004                 0.974                  auto_fast      fast        deferred_extmarks  50000       0.050                 1
auto  long_running_repetition  1896.416         0.906               1.001                 0.925                  auto_full      full        exact              12000       0.100                 0
auto  extmark_heavy            1791.657         0.992               1.028                 1.004                  auto_fast      fast        deferred_extmarks  4000        0.100                 16
auto  conceal_heavy            1848.016         0.890               1.022                 0.927                  auto_fast      fast        deferred_extmarks  4000        0.050                 32
auto  particles_on             1527.379         1.099               1.156                 1.137                  auto_full      full        exact              4000        0.100                 0
full  large_line_count         1571.106         1.069               1.079                 1.055                  full_full      full        exact              50000       0.100                 1
full  long_running_repetition  1728.501         0.934               1.055                 0.925                  full_full      full        exact              12000       0.100                 0
full  extmark_heavy            1680.951         1.009               1.063                 0.958                  full_full      full        exact              4000        0.100                 16
full  conceal_heavy            1693.011         1.004               1.012                 0.999                  full_full      full        exact              4000        0.100                 32
full  particles_on             1711.469         1.016               1.015                 0.995                  full_full      full        exact              4000        0.150                 0
fast  large_line_count         1730.295         0.980               1.005                 0.982                  fast_fast      fast        deferred_extmarks  50000       0.100                 1
fast  long_running_repetition  1730.589         0.973               1.001                 0.930                  fast_fast      fast        deferred_extmarks  12000       0.150                 0
fast  extmark_heavy            1713.406         0.987               1.015                 0.995                  fast_fast      fast        deferred_extmarks  4000        0.050                 16
fast  conceal_heavy            1624.858         0.969               1.040                 0.986                  fast_fast      fast        deferred_extmarks  4000        0.050                 32
fast  particles_on             1532.000         1.036               1.019                 1.008                  fast_fast      fast        deferred_extmarks  4000        0.050                 0
```

### Adaptive Deltas

```text
scenario                 auto_avg_baseline_us  full_avg_baseline_us  fast_avg_baseline_us  auto_vs_full_pct  auto_vs_fast_pct  auto_avg_recovery_ratio  full_avg_recovery_ratio  fast_avg_recovery_ratio  auto_avg_stress_max_ratio  full_avg_stress_max_ratio  fast_avg_stress_max_ratio  auto_class  auto_probe         auto_reason_bits
conceal_heavy            1848.016              1693.011              1624.858              +9.16%            +13.73%           0.890                    1.004                    0.969                    1.022                      1.012                      1.040                      fast        deferred_extmarks  32
extmark_heavy            1791.657              1680.951              1713.406              +6.59%            +4.57%            0.992                    1.009                    0.987                    1.028                      1.063                      1.015                      fast        deferred_extmarks  16
large_line_count         1861.530              1571.106              1730.295              +18.49%           +7.58%            0.954                    1.069                    0.980                    1.004                      1.079                      1.005                      fast        deferred_extmarks  1
long_running_repetition  1896.416              1728.501              1730.589              +9.71%            +9.58%            0.906                    0.934                    0.973                    1.001                      1.055                      1.001                      full        exact              0
particles_on             1527.379              1711.469              1532.000              -10.76%           -0.30%            1.099                    1.016                    1.036                    1.156                      1.015                      1.019                      full        exact              0
```

### Probe Cost Signals

```text
mode  scenario                 avg_extmark_fallback_calls  avg_conceal_full_scan_calls
auto  large_line_count         0.00                        0.00
auto  long_running_repetition  0.00                        0.00
auto  extmark_heavy            6301.00                     0.00
auto  conceal_heavy            0.00                        6300.00
auto  particles_on             0.00                        0.00
full  large_line_count         0.00                        0.00
full  long_running_repetition  0.00                        0.00
full  extmark_heavy            6301.00                     0.00
full  conceal_heavy            0.00                        6300.00
full  particles_on             0.00                        0.00
fast  large_line_count         0.00                        0.00
fast  long_running_repetition  0.00                        0.00
fast  extmark_heavy            6301.00                     0.00
fast  conceal_heavy            0.00                        6300.00
fast  particles_on             0.00                        0.00
```

## Planner Compile

### Raw Results

```text
mode         scenario       run  baseline_us        recovery_ratio      stress_max_avg_us  stress_max_ratio   stress_tail_ratio   perf_class  line_count  planner_bucket_maps_scanned  planner_bucket_cells_scanned  planner_local_query_envelope_area_cells  planner_local_query_cells  planner_compiled_query_cells  planner_candidate_query_cells  planner_compiled_cells_emitted  planner_candidate_cells_built  planner_reference_compiles  planner_local_query_compiles  realized_path
reference    planner_heavy  1    1757.752846666667  0.9673695593161644  1797.378958333333  1.022543619679986  1.022543619679986   full        12000       30543                        534932                        0                                        120123                     59949                         60174                          157576                          781674                         1851                        0                             reference
reference    planner_heavy  2    1796.267708333333  0.9392228603638216  1796.267708333333  1.0                0.9931279188270588  full        12000       32235                        573084                        0                                        119317                     59558                         59759                          154535                          778749                         1819                        0                             reference
local_query  planner_heavy  1    1598.701526666667  1.03673419585859    1776.717535        1.111350371138071  1.101323787751934   full        12000       30419                        534756                        1843713                                  120208                     60017                         60191                          239555                          775315                         0                           2796                          local_query
local_query  planner_heavy  2    1757.659653333333  0.9625069222730218  1783.316006666667  1.014596883580207  1.003124676321485   full        12000       36133                        662248                        1827793                                  127426                     63536                         63890                          238236                          767749                         0                           2800                          local_query
```

### Summary

```text
mode         scenario       avg_baseline_us  avg_recovery_ratio  avg_stress_max_ratio  avg_stress_tail_ratio  perf_class  line_count  realized_path
reference    planner_heavy  1777.010         0.953               1.011                 1.008                  full        12000       reference
local_query  planner_heavy  1678.181         1.000               1.063                 1.052                  full        12000       local_query
```

### Worst-Case Spikes

```text
mode         scenario       worst_baseline_us  worst_recovery_ratio  worst_stress_max_avg_us  worst_stress_max_ratio  worst_stress_tail_ratio  realized_path
reference    planner_heavy  1796.268           0.967                 1797.379                 1.023                   1.023                    reference
local_query  planner_heavy  1757.660           1.037                 1783.316                 1.111                   1.101                    local_query
```

### Planner Telemetry

```text
mode         scenario       avg_planner_bucket_maps_scanned  max_planner_bucket_maps_scanned  avg_planner_bucket_cells_scanned  max_planner_bucket_cells_scanned  avg_planner_local_query_envelope_area_cells  max_planner_local_query_envelope_area_cells  avg_planner_local_query_cells  max_planner_local_query_cells  avg_planner_compiled_query_cells  max_planner_compiled_query_cells  avg_planner_candidate_query_cells  max_planner_candidate_query_cells  avg_planner_compiled_cells_emitted  max_planner_compiled_cells_emitted  avg_planner_candidate_cells_built  max_planner_candidate_cells_built  avg_planner_reference_compiles  avg_planner_local_query_compiles  realized_path
reference    planner_heavy  31389.00                         32235                            554008.00                         573084                            0.00                                         0                                            119720.00                      120123                         59753.50                          59949                             59966.50                           60174                              156055.50                           157576                              780211.50                          781674                             1835.00                         0.00                              reference
local_query  planner_heavy  33276.00                         36133                            598502.00                         662248                            1835753.00                                   1843713                                      123817.00                      127426                         61776.50                          63536                             62040.50                           63890                              238895.50                           239555                              771532.00                          775315                             0.00                            2798.00                           local_query
```

### Compile Deltas

```text
scenario       reference_avg_baseline_us  local_query_avg_baseline_us  local_query_vs_reference_pct  reference_avg_recovery_ratio  local_query_avg_recovery_ratio  reference_avg_stress_max_ratio  local_query_avg_stress_max_ratio  reference_avg_planner_bucket_maps_scanned  local_query_avg_planner_bucket_maps_scanned  reference_avg_planner_bucket_cells_scanned  local_query_avg_planner_bucket_cells_scanned  reference_avg_planner_compiled_cells_emitted  local_query_avg_planner_compiled_cells_emitted  reference_avg_planner_candidate_cells_built  local_query_avg_planner_candidate_cells_built  reference_avg_planner_reference_compiles  local_query_avg_planner_local_query_compiles
planner_heavy  1777.010                   1678.181                     -5.56%                        0.953                         1.000                           1.011                           1.063                             31389.00                                   33276.00                                     554008.00                                   598502.00                                     156055.50                                     238895.50                                       780211.50                                    771532.00                                      1835.00                                   2798.00
```

## Particle Toggle

### Raw Results

```text
side   case           run  avg_us             avg_particles  max_particles  final_particles
local  particles_off  1    8.75859375         0.0            0              0
local  particles_off  2    6.619166666666667  0.0            0              0
local  particles_on   1    18.76824666666667  13.1           33             7
local  particles_on   2    18.41927083333333  13.1           33             7
base   particles_off  1    6.502847083333333  0.0            0              0
base   particles_off  2    6.34107625         0.0            0              0
base   particles_on   1    18.66350708333333  13.1           33             7
base   particles_on   2    18.51730875        13.1           33             7
```

### Summary

```text
side   case           avg_step_us  avg_particles  max_particles  avg_final_particles
local  particles_off  7.689        0.000          0              0.00
local  particles_on   18.594       13.100         33             7.00
base   particles_off  6.422        0.000          0              0.00
base   particles_on   18.590       13.100         33             7.00
```

### Worst-Case Repeats

```text
side   case           worst_step_us  worst_avg_particles  max_particles  worst_final_particles
local  particles_off  8.759          0.000                0              0
local  particles_on   18.768         13.100               33             7
base   particles_off  6.503          0.000                0              0
base   particles_on   18.664         13.100               33             7
```

### Particle Isolation (same side)

```text
side   particles_off_avg_step_us  particles_on_avg_step_us  particle_tax_pct  particles_on_avg_particles  particles_on_max_particles
local  7.689                      18.594                    +141.83%          13.100                      33
base   6.422                      18.590                    +189.48%          13.100                      33
```

### Delta (local vs base)

```text
case           local_avg_step_us  base_avg_step_us  delta_pct  local_avg_particles  base_avg_particles
particles_off  7.689              6.422             +19.73%    0.000                0.000
particles_on   18.594             18.590            +0.02%     13.100               13.100
```

## Window Pool Cap

### Raw Results

```text
side   scenario                 run  baseline_us        recovery_ratio      stress_max_avg_us  stress_max_ratio   stress_tail_ratio   perf_class  line_count  extmark_fallback_calls  conceal_full_scan_calls  planner_bucket_maps_scanned  planner_bucket_cells_scanned  planner_local_query_envelope_area_cells  planner_local_query_cells  planner_compiled_query_cells  planner_candidate_query_cells  planner_compiled_cells_emitted  planner_candidate_cells_built  projection_reuse_hits  projection_reuse_misses  planner_cache_hits  planner_cache_misses  validation_planner_compiled_cells_emitted  validation_planner_candidate_cells_built  pool_total_windows  pool_cached_budget  max_kept_windows  pool_peak_requested_capacity  pool_capacity_cap_hits  win_enter_dropped  win_enter_continued  win_scrolled_dropped  win_scrolled_continued  buf_enter_dropped  buf_enter_continued
local  large_line_count         1    1662.207431666667  1.061811043982428   1714.900243333333  1.031700502995485  1.016322149079069   fast        50000       0                       0                        15548                        116653                        658035                                   45833                      22726                         23107                          32998                           127874                         na                     na                       na                  na                    na                                         na                                        18                  32                  64                18                            0                       na                 na                   na                    na                      na                 na
local  large_line_count         2    1733.023681666667  0.9512946634488624  1733.023681666667  1.0                0.988684095775806   fast        50000       0                       0                        14048                        101332                        672166                                   40362                      20018                         20344                          32768                           130144                         na                     na                       na                  na                    na                                         na                                        18                  32                  64                18                            0                       na                 na                   na                    na                      na                 na
local  long_running_repetition  1    1656.213055        1.020873547375019   1720.420763333334  1.038767783009254  0.9976239097652404  full        12000       0                       0                        22104                        154679                        1367410                                  69633                      34564                         35069                          65827                           271592                         na                     na                       na                  na                    na                                         na                                        18                  32                  64                18                            0                       na                 na                   na                    na                      na                 na
local  long_running_repetition  2    1726.81014         0.9414385040615988  1763.373195        1.021173755095045  0.914639291207776   full        12000       0                       0                        24268                        181670                        1430177                                  73856                      36656                         37200                          68264                           279747                         na                     na                       na                  na                    na                                         na                                        20                  32                  64                20                            0                       na                 na                   na                    na                      na                 na
local  planner_heavy            1    1687.644375        0.9473439282293502  1735.1296525       1.028137016425632  1.026793304227181   full        12000       0                       0                        33517                        616409                        1897575                                  123735                     61677                         62058                          239070                          789577                         na                     na                       na                  na                    na                                         na                                        16                  32                  64                16                            0                       na                 na                   na                    na                      na                 na
local  planner_heavy            2    1539.558471666667  1.093322751930166   1805.4458675       1.172703668439104  1.163573525874193   full        12000       0                       0                        27109                        466949                        1879579                                  110050                     54967                         55083                          243325                          785754                         na                     na                       na                  na                    na                                         na                                        13                  32                  64                13                            0                       na                 na                   na                    na                      na                 na
local  extmark_heavy            1    1435.513333333333  1.034509093146702   1546.429270833333  1.077265696475593  1.077265696475593   fast        4000        6301                    0                        15012                        114569                        609235                                   42966                      21278                         21688                          25747                           114573                         na                     na                       na                  na                    na                                         na                                        18                  32                  64                18                            0                       na                 na                   na                    na                      na                 na
local  extmark_heavy            2    1569.053958333333  0.9786809349954638  1612.807360833333  1.027885212148138  1.027885212148138   fast        4000        6301                    0                        15152                        123900                        687120                                   40607                      20124                         20483                          30869                           127752                         na                     na                       na                  na                    na                                         na                                        17                  32                  64                17                            0                       na                 na                   na                    na                      na                 na
local  conceal_heavy            1    1544.364028333333  0.9515665432753856  1570.7228475       1.017067750014297  0.9714699394972204  fast        4000        0                       6300                     14983                        118936                        689688                                   41740                      20664                         21076                          29995                           125713                         na                     na                       na                  na                    na                                         na                                        21                  32                  64                21                            0                       na                 na                   na                    na                      na                 na
local  conceal_heavy            2    1431.602083333333  1.056725298143543   1594.05191         1.113474147989796  1.113474147989796   fast        4000        0                       6300                     15389                        120849                        693361                                   41448                      20571                         20877                          30476                           124919                         na                     na                       na                  na                    na                                         na                                        19                  32                  64                19                            0                       na                 na                   na                    na                      na                 na
base   large_line_count         1    1572.633611666667  1.018317026580742   1583.723020833333  1.00705148935162   0.9982321761750212  fast        50000       0                       0                        13770                        104938                        719845                                   39869                      19765                         20104                          33137                           136777                         na                     na                       na                  na                    na                                         na                                        17                  32                  384               17                            0                       na                 na                   na                    na                      na                 na
base   large_line_count         2    1537.381805        1.029517626343097   1558.828923333333  1.013950417692977  1.002292475962621   fast        50000       0                       0                        11342                        80047                         699960                                   33590                      16669                         16921                          30655                           130375                         na                     na                       na                  na                    na                                         na                                        17                  32                  384               17                            0                       na                 na                   na                    na                      na                 na
base   long_running_repetition  1    1508.842985        1.059585299835998   1643.76066         1.089417968828612  1.089417968828612   full        12000       0                       0                        21568                        159379                        1437260                                  64950                      32276                         32674                          63211                           276376                         na                     na                       na                  na                    na                                         na                                        20                  32                  384               20                            0                       na                 na                   na                    na                      na                 na
base   long_running_repetition  2    1724.987848333333  0.9337557719935592  1724.987848333333  1.0                0.9763964313105128  full        12000       0                       0                        25188                        205128                        1426090                                  75352                      37388                         37964                          67296                           279881                         na                     na                       na                  na                    na                                         na                                        19                  32                  384               19                            0                       na                 na                   na                    na                      na                 na
base   planner_heavy            1    1461.331666666667  1.149436711264497   1751.2878125       1.198419121714327  1.198419121714327   full        12000       0                       0                        29039                        505927                        1852198                                  111852                     55844                         56008                          236855                          779345                         na                     na                       na                  na                    na                                         na                                        17                  32                  384               17                            0                       na                 na                   na                    na                      na                 na
base   planner_heavy            2    1657.500486666667  1.013292605448526   1782.399965833333  1.075354113118752  1.075354113118752   full        12000       0                       0                        30013                        523375                        1829261                                  118317                     59052                         59265                          235225                          766862                         na                     na                       na                  na                    na                                         na                                        17                  32                  384               17                            0                       na                 na                   na                    na                      na                 na
base   extmark_heavy            1    1591.90743         0.9917444864659416  1591.90743         1.0                0.9859399105679572  fast        4000        6301                    0                        14528                        114513                        677685                                   40068                      19882                         20186                          30956                           126117                         na                     na                       na                  na                    na                                         na                                        20                  32                  384               20                            0                       na                 na                   na                    na                      na                 na
base   extmark_heavy            2    1584.94625         1.049617876105683   1584.94625         1.0                0.9904637496907756  fast        4000        6301                    0                        14510                        114402                        679473                                   39784                      19714                         20070                          31218                           126826                         na                     na                       na                  na                    na                                         na                                        16                  32                  384               16                            0                       na                 na                   na                    na                      na                 na
base   conceal_heavy            1    1498.293473333333  0.9965925111306964  1509.827325833333  1.007697992886761  0.9757707903606476  fast        4000        0                       6300                     16151                        132380                        687699                                   43986                      21785                         22201                          29121                           127487                         na                     na                       na                  na                    na                                         na                                        20                  32                  384               20                            0                       na                 na                   na                    na                      na                 na
base   conceal_heavy            2    1443.369861666667  1.125294557643395   1522.278229166667  1.054669540770987  1.054669540770987   fast        4000        0                       6300                     13873                        110666                        706094                                   38177                      18922                         19255                          29733                           129250                         na                     na                       na                  na                    na                                         na                                        24                  32                  384               24                            0                       na                 na                   na                    na                      na                 na
```

### Summary

```text
side   scenario                 avg_baseline_us  avg_recovery_ratio  avg_stress_max_ratio  avg_stress_tail_ratio  avg_extmark_fallback_calls  avg_conceal_full_scan_calls  perf_class  line_count
local  large_line_count         1697.616         1.007               1.016                 1.003                  0.00                        0.00                         fast        50000
local  long_running_repetition  1691.512         0.981               1.030                 0.956                  0.00                        0.00                         full        12000
local  planner_heavy            1613.601         1.020               1.100                 1.095                  0.00                        0.00                         full        12000
local  extmark_heavy            1502.284         1.007               1.053                 1.053                  6301.00                     0.00                         fast        4000
local  conceal_heavy            1487.983         1.004               1.065                 1.042                  0.00                        6300.00                      fast        4000
base   large_line_count         1555.008         1.024               1.011                 1.000                  0.00                        0.00                         fast        50000
base   long_running_repetition  1616.915         0.997               1.045                 1.033                  0.00                        0.00                         full        12000
base   planner_heavy            1559.416         1.081               1.137                 1.137                  0.00                        0.00                         full        12000
base   extmark_heavy            1588.427         1.021               1.000                 0.988                  6301.00                     0.00                         fast        4000
base   conceal_heavy            1470.832         1.061               1.031                 1.015                  0.00                        6300.00                      fast        4000
```

### Worst-Case Spikes

```text
side   scenario                 worst_baseline_us  worst_recovery_ratio  worst_stress_max_avg_us  worst_stress_max_ratio  worst_stress_tail_ratio  perf_class  line_count
local  large_line_count         1733.024           1.062                 1733.024                 1.032                   1.016                    fast        50000
local  long_running_repetition  1726.810           1.021                 1763.373                 1.039                   0.998                    full        12000
local  planner_heavy            1687.644           1.093                 1805.446                 1.173                   1.164                    full        12000
local  extmark_heavy            1569.054           1.035                 1612.807                 1.077                   1.077                    fast        4000
local  conceal_heavy            1544.364           1.057                 1594.052                 1.113                   1.113                    fast        4000
base   large_line_count         1572.634           1.030                 1583.723                 1.014                   1.002                    fast        50000
base   long_running_repetition  1724.988           1.060                 1724.988                 1.089                   1.089                    full        12000
base   planner_heavy            1657.500           1.149                 1782.400                 1.198                   1.198                    full        12000
base   extmark_heavy            1591.907           1.050                 1591.907                 1.000                   0.990                    fast        4000
base   conceal_heavy            1498.293           1.125                 1522.278                 1.055                   1.055                    fast        4000
```

### Planner Telemetry

```text
side   scenario                 avg_planner_bucket_maps_scanned  max_planner_bucket_maps_scanned  avg_planner_bucket_cells_scanned  max_planner_bucket_cells_scanned  avg_planner_local_query_envelope_area_cells  max_planner_local_query_envelope_area_cells  avg_planner_local_query_cells  max_planner_local_query_cells  avg_planner_compiled_query_cells  max_planner_compiled_query_cells  avg_planner_candidate_query_cells  max_planner_candidate_query_cells  avg_planner_compiled_cells_emitted  max_planner_compiled_cells_emitted  avg_planner_candidate_cells_built  max_planner_candidate_cells_built
local  large_line_count         14798.00                         15548                            108992.50                         116653                            665100.50                                    672166                                       43097.50                       45833                          21372.00                          22726                             21725.50                           23107                              32883.00                            32998                               129009.00                          130144
local  long_running_repetition  23186.00                         24268                            168174.50                         181670                            1398793.50                                   1430177                                      71744.50                       73856                          35610.00                          36656                             36134.50                           37200                              67045.50                            68264                               275669.50                          279747
local  planner_heavy            30313.00                         33517                            541679.00                         616409                            1888577.00                                   1897575                                      116892.50                      123735                         58322.00                          61677                             58570.50                           62058                              241197.50                           243325                              787665.50                          789577
local  extmark_heavy            15082.00                         15152                            119234.50                         123900                            648177.50                                    687120                                       41786.50                       42966                          20701.00                          21278                             21085.50                           21688                              28308.00                            30869                               121162.50                          127752
local  conceal_heavy            15186.00                         15389                            119892.50                         120849                            691524.50                                    693361                                       41594.00                       41740                          20617.50                          20664                             20976.50                           21076                              30235.50                            30476                               125316.00                          125713
base   large_line_count         12556.00                         13770                            92492.50                          104938                            709902.50                                    719845                                       36729.50                       39869                          18217.00                          19765                             18512.50                           20104                              31896.00                            33137                               133576.00                          136777
base   long_running_repetition  23378.00                         25188                            182253.50                         205128                            1431675.00                                   1437260                                      70151.00                       75352                          34832.00                          37388                             35319.00                           37964                              65253.50                            67296                               278128.50                          279881
base   planner_heavy            29526.00                         30013                            514651.00                         523375                            1840729.50                                   1852198                                      115084.50                      118317                         57448.00                          59052                             57636.50                           59265                              236040.00                           236855                              773103.50                          779345
base   extmark_heavy            14519.00                         14528                            114457.50                         114513                            678579.00                                    679473                                       39926.00                       40068                          19798.00                          19882                             20128.00                           20186                              31087.00                            31218                               126471.50                          126826
base   conceal_heavy            15012.00                         16151                            121523.00                         132380                            696896.50                                    706094                                       41081.50                       43986                          20353.50                          21785                             20728.00                           22201                              29427.00                            29733                               128368.50                          129250
```

### Pool Retention vs max_kept_windows

```text
side   scenario                 avg_pool_total_windows  avg_pool_cached_budget  max_kept_windows  avg_pool_total_pct_of_max  avg_pool_cached_budget_pct_of_max
local  large_line_count         18.00                   32.00                   64                28.12                      50.00
local  long_running_repetition  19.00                   32.00                   64                29.69                      50.00
local  planner_heavy            14.50                   32.00                   64                22.66                      50.00
local  extmark_heavy            17.50                   32.00                   64                27.34                      50.00
local  conceal_heavy            20.00                   32.00                   64                31.25                      50.00
base   large_line_count         17.00                   32.00                   384               4.43                       8.33
base   long_running_repetition  19.50                   32.00                   384               5.08                       8.33
base   planner_heavy            17.00                   32.00                   384               4.43                       8.33
base   extmark_heavy            18.00                   32.00                   384               4.69                       8.33
base   conceal_heavy            22.00                   32.00                   384               5.73                       8.33
```

### Pool Peak Pressure vs max_kept_windows

```text
side   scenario                 avg_pool_peak_requested_capacity  max_pool_capacity_cap_hits  max_kept_windows  avg_pool_peak_requested_pct_of_max
local  large_line_count         18.00                             0                           64                28.12
local  long_running_repetition  19.00                             0                           64                29.69
local  planner_heavy            14.50                             0                           64                22.66
local  extmark_heavy            17.50                             0                           64                27.34
local  conceal_heavy            20.00                             0                           64                31.25
base   large_line_count         17.00                             0                           384               4.43
base   long_running_repetition  19.50                             0                           384               5.08
base   planner_heavy            17.00                             0                           384               4.43
base   extmark_heavy            18.00                             0                           384               4.69
base   conceal_heavy            22.00                             0                           384               5.73
```

### Delta (local vs base)

```text
scenario                 local_avg_baseline_us  base_avg_baseline_us  baseline_delta_pct  local_avg_recovery_ratio  base_avg_recovery_ratio  local_avg_stress_max_ratio  base_avg_stress_max_ratio  local_avg_stress_tail_ratio  base_avg_stress_tail_ratio
large_line_count         1697.616               1555.008              +9.17%              1.007                     1.024                    1.016                       1.011                      1.003                        1.000
long_running_repetition  1691.512               1616.915              +4.61%              0.981                     0.997                    1.030                       1.045                      0.956                        1.033
planner_heavy            1613.601               1559.416              +3.47%              1.020                     1.081                    1.100                       1.137                      1.095                        1.137
extmark_heavy            1502.284               1588.427              -5.42%              1.007                     1.021                    1.053                       1.000                      1.053                        0.988
conceal_heavy            1487.983               1470.832              +1.17%              1.004                     1.061                    1.065                       1.031                      1.042                        1.015
```


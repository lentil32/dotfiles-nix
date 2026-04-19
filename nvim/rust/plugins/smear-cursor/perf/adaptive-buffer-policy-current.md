# Adaptive Buffer Perf Snapshot

- Captured (UTC): 2026-04-10T10:26:39Z
- Repo commit: `5f01957cb6fa64a93ba15e5f98c6bb8a46c98c33`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.7`
- Command: `SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/adaptive-buffer-policy-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/compare_buffer_perf_modes.sh`
- Config: repeats=`2`, modes=`auto,full,fast`, scenarios=`large_line_count,long_running_repetition,extmark_heavy,conceal_heavy`, warmup=`300`, baseline=`600`, stress=`1200`, stress_rounds=`preset`, recovery=`600`, recovery_mode=`fixed`, settle_wait_ms=`250`, windows=`8`, drain_every=`1`, delay_event_to_smear=`0`

## Raw Results

```text
mode  scenario                 run  baseline_us  recovery_ratio  stress_max_avg_us  stress_max_ratio  stress_tail_ratio  realized_mode  perf_class  probe_policy             line_count  callback_ewma_ms  reason_bits  extmark_fallback_calls  conceal_full_scan_calls
auto  large_line_count         1    1633.010     0.995           1633.010           1.000             0.966              auto_fast      fast        raw_extmarks             50000       0.0               65           0                       0
auto  large_line_count         2    1455.623     1.144           1581.536           1.087             0.984              auto_fast      fast        raw_extmarks             50000       0.1               65           0                       0
auto  long_running_repetition  1    1514.029     1.058           1622.809           1.072             1.055              auto_full      full        exact                    12000       0.1               0            0                       0
auto  long_running_repetition  2    1568.655     1.034           1604.632           1.023             1.019              auto_full      full        exact                    12000       0.1               0            0                       0
auto  extmark_heavy            1    1623.052     1.039           1638.907           1.010             1.010              auto_fast      fast        raw_compatible_extmarks  4000        0.0               80           6301                    0
auto  extmark_heavy            2    1555.436     1.117           1625.876           1.045             1.045              auto_fast      fast        raw_compatible_extmarks  4000        0.1               80           6301                    0
auto  conceal_heavy            1    1668.128     1.007           1686.352           1.011             1.004              auto_fast      fast        raw_extmarks             4000        0.1               64           0                       4
auto  conceal_heavy            2    1626.843     0.872           1626.843           1.000             0.922              auto_fast      fast        raw_extmarks             4000        0.0               64           0                       4
full  large_line_count         1    1408.886     1.134           1532.140           1.087             1.087              full_full      full        exact                    50000       0.1               1            0                       0
full  large_line_count         2    1527.716     0.981           1556.441           1.019             0.974              full_full      full        exact                    50000       0.1               1            0                       0
full  long_running_repetition  1    1392.745     1.126           1567.755           1.126             1.117              full_full      full        exact                    12000       0.0               0            0                       0
full  long_running_repetition  2    1522.044     1.034           1593.438           1.047             1.030              full_full      full        exact                    12000       0.1               0            0                       0
full  extmark_heavy            1    1571.652     1.020           1608.933           1.024             1.024              full_full      full        exact_compatible         4000        0.1               16           6301                    0
full  extmark_heavy            2    1561.650     1.063           1646.324           1.054             1.054              full_full      full        exact_compatible         4000        0.1               16           6301                    0
full  conceal_heavy            1    1558.618     1.088           1719.801           1.103             1.085              full_full      full        exact                    4000        0.1               32           0                       6300
full  conceal_heavy            2    1579.909     1.047           1630.458           1.032             1.019              full_full      full        exact                    4000        0.0               32           0                       6300
fast  large_line_count         1    1522.933     1.061           1618.387           1.063             1.063              fast_fast      fast        raw_extmarks             50000       0.0               65           0                       0
fast  large_line_count         2    1568.737     1.040           1583.178           1.009             0.954              fast_fast      fast        raw_extmarks             50000       0.1               65           0                       0
fast  long_running_repetition  1    1521.073     1.059           1657.719           1.090             1.062              fast_fast      fast        raw_extmarks             12000       0.1               64           0                       0
fast  long_running_repetition  2    1539.974     1.042           1628.423           1.057             1.048              fast_fast      fast        raw_extmarks             12000       0.1               64           0                       0
fast  extmark_heavy            1    1625.783     0.979           1668.293           1.026             0.904              fast_fast      fast        raw_compatible_extmarks  4000        0.1               80           6301                    0
fast  extmark_heavy            2    1569.978     1.037           1582.287           1.008             0.942              fast_fast      fast        raw_compatible_extmarks  4000        0.0               80           6301                    0
fast  conceal_heavy            1    1449.558     1.088           1551.905           1.071             1.032              fast_fast      fast        raw_extmarks             4000        0.1               64           0                       2
fast  conceal_heavy            2    1542.466     1.022           1584.761           1.027             1.027              fast_fast      fast        raw_extmarks             4000        0.1               64           0                       2
```

## Summary

```text
mode  scenario                 avg_baseline_us  avg_recovery_ratio  avg_stress_max_ratio  avg_stress_tail_ratio  realized_mode  perf_class  probe_policy             line_count  avg_callback_ewma_ms  reason_bits
auto  large_line_count         1544.316         1.069               1.043                 0.975                  auto_fast      fast        raw_extmarks             50000       0.050                 65
auto  long_running_repetition  1541.342         1.046               1.047                 1.037                  auto_full      full        exact                    12000       0.100                 0
auto  extmark_heavy            1589.244         1.078               1.027                 1.027                  auto_fast      fast        raw_compatible_extmarks  4000        0.050                 80
auto  conceal_heavy            1647.486         0.940               1.006                 0.963                  auto_fast      fast        raw_extmarks             4000        0.050                 64
full  large_line_count         1468.301         1.057               1.053                 1.030                  full_full      full        exact                    50000       0.100                 1
full  long_running_repetition  1457.394         1.080               1.087                 1.074                  full_full      full        exact                    12000       0.050                 0
full  extmark_heavy            1566.651         1.042               1.039                 1.039                  full_full      full        exact_compatible         4000        0.100                 16
full  conceal_heavy            1569.264         1.067               1.067                 1.052                  full_full      full        exact                    4000        0.050                 32
fast  large_line_count         1545.835         1.050               1.036                 1.008                  fast_fast      fast        raw_extmarks             50000       0.050                 65
fast  long_running_repetition  1530.524         1.050               1.074                 1.055                  fast_fast      fast        raw_extmarks             12000       0.100                 64
fast  extmark_heavy            1597.880         1.008               1.017                 0.923                  fast_fast      fast        raw_compatible_extmarks  4000        0.050                 80
fast  conceal_heavy            1496.012         1.055               1.049                 1.030                  fast_fast      fast        raw_extmarks             4000        0.100                 64
```

## Adaptive Deltas

```text
scenario                 auto_avg_baseline_us  full_avg_baseline_us  fast_avg_baseline_us  auto_vs_full_pct  auto_vs_fast_pct  auto_avg_recovery_ratio  full_avg_recovery_ratio  fast_avg_recovery_ratio  auto_avg_stress_max_ratio  full_avg_stress_max_ratio  fast_avg_stress_max_ratio  auto_class  auto_probe               auto_reason_bits
conceal_heavy            1647.486              1569.264              1496.012              +4.98%            +10.13%           0.940                    1.067                    1.055                    1.006                      1.067                      1.049                      fast        raw_extmarks             64
extmark_heavy            1589.244              1566.651              1597.880              +1.44%            -0.54%            1.078                    1.042                    1.008                    1.027                      1.039                      1.017                      fast        raw_compatible_extmarks  80
large_line_count         1544.316              1468.301              1545.835              +5.18%            -0.10%            1.069                    1.057                    1.050                    1.043                      1.053                      1.036                      fast        raw_extmarks             65
long_running_repetition  1541.342              1457.394              1530.524              +5.76%            +0.71%            1.046                    1.080                    1.050                    1.047                      1.087                      1.074                      full        exact                    0
```

## Probe Cost Signals

```text
mode  scenario                 avg_extmark_fallback_calls  avg_conceal_full_scan_calls
auto  large_line_count         0.00                        0.00
auto  long_running_repetition  0.00                        0.00
auto  extmark_heavy            6301.00                     0.00
auto  conceal_heavy            0.00                        4.00
full  large_line_count         0.00                        0.00
full  long_running_repetition  0.00                        0.00
full  extmark_heavy            6301.00                     0.00
full  conceal_heavy            0.00                        6300.00
fast  large_line_count         0.00                        0.00
fast  long_running_repetition  0.00                        0.00
fast  extmark_heavy            6301.00                     0.00
fast  conceal_heavy            0.00                        2.00
```

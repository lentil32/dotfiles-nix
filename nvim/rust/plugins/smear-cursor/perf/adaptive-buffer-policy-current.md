# Adaptive Buffer Perf Snapshot

- Captured (UTC): 2026-03-27T05:01:09Z
- Repo commit: `91d00093a179d32eaf86ad534ecc117e74c2fb23`
- Working tree: `clean`
- Neovim: `NVIM v0.11.6`
- Command: `SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/adaptive-buffer-policy-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/compare_buffer_perf_modes.sh`
- Config: repeats=`2`, modes=`auto,full,fast`, scenarios=`large_line_count,long_running_repetition,extmark_heavy,conceal_heavy`, warmup=`300`, baseline=`600`, stress=`1200`, stress_rounds=`preset`, recovery=`600`, recovery_mode=`fixed`, settle_wait_ms=`250`, windows=`8`, drain_every=`1`, delay_event_to_smear=`0`

## Raw Results

```text
mode  scenario                 run  baseline_us  recovery_ratio  stress_max_avg_us  stress_max_ratio  stress_tail_ratio  realized_mode  perf_class  probe_policy      line_count  callback_ewma_ms  reason_bits  extmark_fallback_calls  conceal_full_scan_calls
auto  large_line_count         1    1606.987     0.994           1614.277           1.005             1.005              auto_fast      fast        raw_syntax        50000       0.0               65           0                       0
auto  large_line_count         2    1605.118     1.017           1619.141           1.009             1.008              auto_fast      fast        raw_syntax        50000       0.1               65           0                       0
auto  long_running_repetition  1    1626.514     1.012           1626.514           1.000             0.986              auto_full      full        exact             12000       0.0               0            0                       0
auto  long_running_repetition  2    1560.935     1.079           1637.620           1.049             1.049              auto_full      full        exact             12000       0.1               0            0                       0
auto  extmark_heavy            1    1628.325     1.034           1671.398           1.026             1.026              auto_fast      fast        raw_syntax        4000        0.0               64           3                       0
auto  extmark_heavy            2    1640.703     1.036           1646.434           1.003             0.993              auto_fast      fast        raw_syntax        4000        0.0               64           3                       0
auto  conceal_heavy            1    1633.651     0.998           1633.651           1.000             0.987              auto_fast      fast        raw_syntax        4000        0.1               64           0                       4
auto  conceal_heavy            2    1554.118     1.117           1616.387           1.040             1.028              auto_fast      fast        raw_syntax        4000        0.0               64           0                       4
full  large_line_count         1    1590.758     1.007           1590.758           1.000             0.989              full_full      full        exact             50000       0.0               1            0                       0
full  large_line_count         2    1567.338     0.985           1585.640           1.012             1.012              full_full      full        exact             50000       0.0               1            0                       0
full  long_running_repetition  1    1617.883     1.018           1627.735           1.006             1.001              full_full      full        exact             12000       0.1               0            0                       0
full  long_running_repetition  2    1596.749     1.016           1627.505           1.019             1.019              full_full      full        exact             12000       0.0               0            0                       0
full  extmark_heavy            1    1601.185     1.027           1622.650           1.013             0.998              full_full      full        exact_compatible  4000        0.0               0            8                       0
full  extmark_heavy            2    1613.462     0.996           1619.019           1.003             0.988              full_full      full        exact_compatible  4000        0.0               0            8                       0
full  conceal_heavy            1    1602.780     1.009           1623.878           1.013             1.013              full_full      full        exact             4000        0.0               0            0                       8
full  conceal_heavy            2    1595.495     0.978           1626.119           1.019             1.019              full_full      full        exact             4000        0.0               0            0                       8
fast  large_line_count         1    1593.719     0.991           1610.254           1.010             0.991              fast_fast      fast        raw_syntax        50000       0.0               65           0                       0
fast  large_line_count         2    1578.462     1.039           1609.713           1.020             1.020              fast_fast      fast        raw_syntax        50000       0.0               65           0                       0
fast  long_running_repetition  1    1630.156     0.957           1632.145           1.001             0.989              fast_fast      fast        raw_syntax        12000       0.2               64           0                       0
fast  long_running_repetition  2    1515.123     1.109           1621.152           1.070             1.055              fast_fast      fast        raw_syntax        12000       0.0               64           0                       0
fast  extmark_heavy            1    1629.601     1.016           1654.110           1.015             1.002              fast_fast      fast        raw_syntax        4000        0.1               64           1                       0
fast  extmark_heavy            2    1647.219     1.036           1655.128           1.005             1.005              fast_fast      fast        raw_syntax        4000        0.0               64           1                       0
fast  conceal_heavy            1    1559.009     0.998           1611.404           1.034             1.034              fast_fast      fast        raw_syntax        4000        0.1               64           0                       3
fast  conceal_heavy            2    1624.928     0.981           1624.928           1.000             0.977              fast_fast      fast        raw_syntax        4000        0.0               64           0                       3
```

## Summary

```text
mode  scenario                 avg_baseline_us  avg_recovery_ratio  avg_stress_max_ratio  avg_stress_tail_ratio  realized_mode  perf_class  probe_policy      line_count  avg_callback_ewma_ms  reason_bits
auto  large_line_count         1606.053         1.006               1.007                 1.006                  auto_fast      fast        raw_syntax        50000       0.050                 65
auto  long_running_repetition  1593.724         1.046               1.024                 1.018                  auto_full      full        exact             12000       0.050                 0
auto  extmark_heavy            1634.514         1.035               1.014                 1.010                  auto_fast      fast        raw_syntax        4000        0.000                 64
auto  conceal_heavy            1593.885         1.058               1.020                 1.008                  auto_fast      fast        raw_syntax        4000        0.050                 64
full  large_line_count         1579.048         0.996               1.006                 1.000                  full_full      full        exact             50000       0.000                 1
full  long_running_repetition  1607.316         1.017               1.012                 1.010                  full_full      full        exact             12000       0.050                 0
full  extmark_heavy            1607.323         1.011               1.008                 0.993                  full_full      full        exact_compatible  4000        0.000                 0
full  conceal_heavy            1599.137         0.993               1.016                 1.016                  full_full      full        exact             4000        0.000                 0
fast  large_line_count         1586.091         1.015               1.015                 1.006                  fast_fast      fast        raw_syntax        50000       0.000                 65
fast  long_running_repetition  1572.639         1.033               1.035                 1.022                  fast_fast      fast        raw_syntax        12000       0.100                 64
fast  extmark_heavy            1638.410         1.026               1.010                 1.003                  fast_fast      fast        raw_syntax        4000        0.050                 64
fast  conceal_heavy            1591.968         0.990               1.017                 1.006                  fast_fast      fast        raw_syntax        4000        0.050                 64
```

## Adaptive Deltas

```text
scenario                 auto_avg_baseline_us  full_avg_baseline_us  fast_avg_baseline_us  auto_vs_full_pct  auto_vs_fast_pct  auto_avg_recovery_ratio  full_avg_recovery_ratio  fast_avg_recovery_ratio  auto_avg_stress_max_ratio  full_avg_stress_max_ratio  fast_avg_stress_max_ratio  auto_class  auto_probe  auto_reason_bits
conceal_heavy            1593.885              1599.137              1591.968              -0.33%            +0.12%            1.058                    0.993                    0.990                    1.020                      1.016                      1.017                      fast        raw_syntax  64
extmark_heavy            1634.514              1607.323              1638.410              +1.69%            -0.24%            1.035                    1.011                    1.026                    1.014                      1.008                      1.010                      fast        raw_syntax  64
large_line_count         1606.053              1579.048              1586.091              +1.71%            +1.26%            1.006                    0.996                    1.015                    1.007                      1.006                      1.015                      fast        raw_syntax  65
long_running_repetition  1593.724              1607.316              1572.639              -0.85%            +1.34%            1.046                    1.017                    1.033                    1.024                      1.012                      1.035                      full        exact       0
```

## Probe Cost Signals

```text
mode  scenario                 avg_extmark_fallback_calls  avg_conceal_full_scan_calls
auto  large_line_count         0.00                        0.00
auto  long_running_repetition  0.00                        0.00
auto  extmark_heavy            3.00                        0.00
auto  conceal_heavy            0.00                        4.00
full  large_line_count         0.00                        0.00
full  long_running_repetition  0.00                        0.00
full  extmark_heavy            8.00                        0.00
full  conceal_heavy            0.00                        8.00
fast  large_line_count         0.00                        0.00
fast  long_running_repetition  0.00                        0.00
fast  extmark_heavy            1.00                        0.00
fast  conceal_heavy            0.00                        3.00
```

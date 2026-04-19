# Adaptive Buffer Perf Snapshot

- Captured (UTC): 2026-04-16T07:40:12Z
- Repo commit: `d425ed25eb26f12bdbdf84a5839b87f8f81579db`
- Working tree: `clean`
- Neovim: `NVIM v0.11.7`
- Command: `SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/adaptive-buffer-policy-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/compare_buffer_perf_modes.sh`
- Config: repeats=`2`, modes=`auto,full,fast`, scenarios=`large_line_count,long_running_repetition,extmark_heavy,conceal_heavy`, warmup=`300`, baseline=`600`, stress=`1200`, stress_rounds=`preset`, recovery=`600`, recovery_mode=`fixed`, settle_wait_ms=`250`, windows=`8`, drain_every=`1`, delay_event_to_smear=`0`

## Raw Results

```text
mode  scenario                 run  baseline_us  recovery_ratio  stress_max_avg_us  stress_max_ratio  stress_tail_ratio  realized_mode  perf_class  probe_policy                  line_count  callback_ewma_ms  reason_bits  extmark_fallback_calls  conceal_full_scan_calls
auto  large_line_count         1    1415.488     1.049           1437.350           1.015             1.015              auto_fast      fast        deferred_extmarks             50000       0.0               1            0                       0
auto  large_line_count         2    1399.486     1.117           1511.433           1.080             1.080              auto_fast      fast        deferred_extmarks             50000       0.1               1            0                       0
auto  long_running_repetition  1    1430.017     1.096           1559.133           1.090             1.079              auto_full      full        exact                         12000       0.1               0            0                       0
auto  long_running_repetition  2    1422.652     1.125           1559.714           1.096             1.096              auto_full      full        exact                         12000       0.0               0            0                       0
auto  extmark_heavy            1    1546.107     1.017           1572.074           1.017             1.017              auto_fast      fast        deferred_compatible_extmarks  4000        0.1               16           6291                    0
auto  extmark_heavy            2    1537.666     0.997           1566.900           1.019             1.019              auto_fast      fast        deferred_compatible_extmarks  4000        0.1               16           6291                    0
auto  conceal_heavy            1    1487.021     1.031           1546.412           1.040             1.040              auto_fast      fast        deferred_extmarks             4000        0.1               32           0                       6292
auto  conceal_heavy            2    1473.002     1.028           1551.649           1.053             1.053              auto_fast      fast        deferred_extmarks             4000        0.0               32           0                       6291
full  large_line_count         1    1460.866     1.038           1490.528           1.020             1.013              full_full      full        exact                         50000       0.0               1            0                       0
full  large_line_count         2    1459.785     1.073           1503.339           1.030             1.023              full_full      full        exact                         50000       0.0               1            0                       0
full  long_running_repetition  1    1435.658     1.112           1559.197           1.086             1.081              full_full      full        exact                         12000       0.0               0            0                       0
full  long_running_repetition  2    1428.596     1.095           1544.576           1.081             1.072              full_full      full        exact                         12000       0.1               0            0                       0
full  extmark_heavy            1    1546.885     1.038           1566.520           1.013             1.013              full_full      full        exact_compatible              4000        0.1               16           6291                    0
full  extmark_heavy            2    1558.232     1.011           1592.194           1.022             1.022              full_full      full        exact_compatible              4000        0.1               16           6291                    0
full  conceal_heavy            1    1478.722     1.028           1523.192           1.030             1.021              full_full      full        exact                         4000        0.0               32           0                       6291
full  conceal_heavy            2    1492.972     1.008           1555.014           1.042             1.035              full_full      full        exact                         4000        0.0               32           0                       6291
fast  large_line_count         1    1434.693     1.077           1541.846           1.075             1.071              fast_fast      fast        deferred_extmarks             50000       0.0               1            0                       0
fast  large_line_count         2    1475.315     1.019           1491.527           1.011             0.972              fast_fast      fast        deferred_extmarks             50000       0.0               1            0                       0
fast  long_running_repetition  1    1434.288     1.050           1550.893           1.081             1.072              fast_fast      fast        deferred_extmarks             12000       0.1               0            0                       0
fast  long_running_repetition  2    1476.557     1.067           1527.720           1.035             1.022              fast_fast      fast        deferred_extmarks             12000       0.1               0            0                       0
fast  extmark_heavy            1    1550.932     1.007           1560.868           1.006             1.006              fast_fast      fast        deferred_compatible_extmarks  4000        0.0               16           6292                    0
fast  extmark_heavy            2    1474.847     1.060           1556.618           1.055             1.034              fast_fast      fast        deferred_compatible_extmarks  4000        0.1               16           6291                    0
fast  conceal_heavy            1    1538.140     1.015           1554.681           1.011             1.011              fast_fast      fast        deferred_extmarks             4000        0.1               32           0                       6290
fast  conceal_heavy            2    1536.741     1.031           1536.741           1.000             0.982              fast_fast      fast        deferred_extmarks             4000        0.0               32           0                       6290
```

## Summary

```text
mode  scenario                 avg_baseline_us  avg_recovery_ratio  avg_stress_max_ratio  avg_stress_tail_ratio  realized_mode  perf_class  probe_policy                  line_count  avg_callback_ewma_ms  reason_bits
auto  large_line_count         1407.487         1.083               1.047                 1.047                  auto_fast      fast        deferred_extmarks             50000       0.050                 1
auto  long_running_repetition  1426.334         1.111               1.093                 1.087                  auto_full      full        exact                         12000       0.050                 0
auto  extmark_heavy            1541.887         1.007               1.018                 1.018                  auto_fast      fast        deferred_compatible_extmarks  4000        0.100                 16
auto  conceal_heavy            1480.012         1.030               1.046                 1.046                  auto_fast      fast        deferred_extmarks             4000        0.050                 32
full  large_line_count         1460.325         1.055               1.025                 1.018                  full_full      full        exact                         50000       0.000                 1
full  long_running_repetition  1432.127         1.103               1.083                 1.077                  full_full      full        exact                         12000       0.050                 0
full  extmark_heavy            1552.559         1.024               1.018                 1.018                  full_full      full        exact_compatible              4000        0.100                 16
full  conceal_heavy            1485.847         1.018               1.036                 1.028                  full_full      full        exact                         4000        0.000                 32
fast  large_line_count         1455.004         1.048               1.043                 1.022                  fast_fast      fast        deferred_extmarks             50000       0.000                 1
fast  long_running_repetition  1455.423         1.058               1.058                 1.047                  fast_fast      fast        deferred_extmarks             12000       0.100                 0
fast  extmark_heavy            1512.889         1.034               1.030                 1.020                  fast_fast      fast        deferred_compatible_extmarks  4000        0.050                 16
fast  conceal_heavy            1537.441         1.023               1.006                 0.996                  fast_fast      fast        deferred_extmarks             4000        0.050                 32
```

## Adaptive Deltas

```text
scenario                 auto_avg_baseline_us  full_avg_baseline_us  fast_avg_baseline_us  auto_vs_full_pct  auto_vs_fast_pct  auto_avg_recovery_ratio  full_avg_recovery_ratio  fast_avg_recovery_ratio  auto_avg_stress_max_ratio  full_avg_stress_max_ratio  fast_avg_stress_max_ratio  auto_class  auto_probe                    auto_reason_bits
conceal_heavy            1480.012              1485.847              1537.441              -0.39%            -3.74%            1.030                    1.018                    1.023                    1.046                      1.036                      1.006                      fast        deferred_extmarks             32
extmark_heavy            1541.887              1552.559              1512.889              -0.69%            +1.92%            1.007                    1.024                    1.034                    1.018                      1.018                      1.030                      fast        deferred_compatible_extmarks  16
large_line_count         1407.487              1460.325              1455.004              -3.62%            -3.27%            1.083                    1.055                    1.048                    1.047                      1.025                      1.043                      fast        deferred_extmarks             1
long_running_repetition  1426.334              1432.127              1455.423              -0.40%            -2.00%            1.111                    1.103                    1.058                    1.093                      1.083                      1.058                      full        exact                         0
```

## Probe Cost Signals

```text
mode  scenario                 avg_extmark_fallback_calls  avg_conceal_full_scan_calls
auto  large_line_count         0.00                        0.00
auto  long_running_repetition  0.00                        0.00
auto  extmark_heavy            6291.00                     0.00
auto  conceal_heavy            0.00                        6291.50
full  large_line_count         0.00                        0.00
full  long_running_repetition  0.00                        0.00
full  extmark_heavy            6291.00                     0.00
full  conceal_heavy            0.00                        6291.00
fast  large_line_count         0.00                        0.00
fast  long_running_repetition  0.00                        0.00
fast  extmark_heavy            6291.50                     0.00
fast  conceal_heavy            0.00                        6290.00
```

# Adaptive Buffer Perf Snapshot

- Captured (UTC): 2026-04-10T10:28:45Z
- Repo commit: `5f01957cb6fa64a93ba15e5f98c6bb8a46c98c33`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.7`
- Command: `SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/particle-degraded-buffer-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/compare_buffer_perf_modes.sh`
- Config: repeats=`2`, modes=`full,fast`, scenarios=`particles_on`, warmup=`300`, baseline=`600`, stress=`1200`, stress_rounds=`preset`, recovery=`600`, recovery_mode=`fixed`, settle_wait_ms=`250`, windows=`8`, drain_every=`1`, delay_event_to_smear=`0`

## Raw Results

```text
mode  scenario      run  baseline_us  recovery_ratio  stress_max_avg_us  stress_max_ratio  stress_tail_ratio  realized_mode  perf_class  probe_policy  line_count  callback_ewma_ms  reason_bits  extmark_fallback_calls  conceal_full_scan_calls
full  particles_on  1    1421.616     1.081           1476.650           1.039             1.035              full_full      full        exact         4000        0.1               0            0                       0
full  particles_on  2    1427.322     1.049           1519.221           1.064             1.015              full_full      full        exact         4000        0.0               0            0                       0
fast  particles_on  1    1387.939     1.128           1554.354           1.120             1.120              fast_fast      fast        raw_extmarks  4000        0.1               64           0                       0
fast  particles_on  2    1518.524     1.037           1557.649           1.026             1.026              fast_fast      fast        raw_extmarks  4000        0.0               64           0                       0
```

## Summary

```text
mode  scenario      avg_baseline_us  avg_recovery_ratio  avg_stress_max_ratio  avg_stress_tail_ratio  realized_mode  perf_class  probe_policy  line_count  avg_callback_ewma_ms  reason_bits
full  particles_on  1424.469         1.065               1.051                 1.025                  full_full      full        exact         4000        0.050                 0
fast  particles_on  1453.231         1.083               1.073                 1.073                  fast_fast      fast        raw_extmarks  4000        0.050                 64
```

## Probe Cost Signals

```text
mode  scenario      avg_extmark_fallback_calls  avg_conceal_full_scan_calls
full  particles_on  0.00                        0.00
fast  particles_on  0.00                        0.00
```

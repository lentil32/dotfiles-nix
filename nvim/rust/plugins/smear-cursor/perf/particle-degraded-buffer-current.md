# Adaptive Buffer Perf Snapshot

- Captured (UTC): 2026-04-03T16:03:59Z
- Repo commit: `8f0843c45d20d55dab22686dd92266a5d6c28649`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.6`
- Command: `SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/particle-degraded-buffer-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/compare_buffer_perf_modes.sh`
- Config: repeats=`2`, modes=`full,fast`, scenarios=`particles_on`, warmup=`300`, baseline=`600`, stress=`1200`, stress_rounds=`preset`, recovery=`600`, recovery_mode=`fixed`, settle_wait_ms=`250`, windows=`8`, drain_every=`1`, delay_event_to_smear=`0`

## Raw Results

```text
mode  scenario      run  baseline_us  recovery_ratio  stress_max_avg_us  stress_max_ratio  stress_tail_ratio  realized_mode  perf_class  probe_policy  line_count  callback_ewma_ms  reason_bits  extmark_fallback_calls  conceal_full_scan_calls
full  particles_on  1    1479.264     1.015           1510.927           1.021             1.021              full_full      full        exact         4000        0.0               0            0                       0
full  particles_on  2    1507.399     1.005           1512.719           1.004             0.982              full_full      full        exact         4000        0.0               0            0                       0
fast  particles_on  1    1481.079     1.004           1514.193           1.022             1.004              fast_fast      fast        raw_extmarks  4000        0.0               64           0                       0
fast  particles_on  2    1442.499     1.081           1524.180           1.057             1.039              fast_fast      fast        raw_extmarks  4000        0.0               64           0                       0
```

## Summary

```text
mode  scenario      avg_baseline_us  avg_recovery_ratio  avg_stress_max_ratio  avg_stress_tail_ratio  realized_mode  perf_class  probe_policy  line_count  avg_callback_ewma_ms  reason_bits
full  particles_on  1493.331         1.010               1.012                 1.002                  full_full      full        exact         4000        0.000                 0
fast  particles_on  1461.789         1.042               1.039                 1.022                  fast_fast      fast        raw_extmarks  4000        0.000                 64
```

## Probe Cost Signals

```text
mode  scenario      avg_extmark_fallback_calls  avg_conceal_full_scan_calls
full  particles_on  0.00                        0.00
fast  particles_on  0.00                        0.00
```

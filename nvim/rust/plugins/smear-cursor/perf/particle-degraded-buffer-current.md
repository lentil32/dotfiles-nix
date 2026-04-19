# Adaptive Buffer Perf Snapshot

- Captured (UTC): 2026-04-16T07:47:27Z
- Repo commit: `d425ed25eb26f12bdbdf84a5839b87f8f81579db`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.7`
- Command: `SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/particle-degraded-buffer-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/compare_buffer_perf_modes.sh`
- Config: repeats=`2`, modes=`full,fast`, scenarios=`particles_on`, warmup=`300`, baseline=`600`, stress=`1200`, stress_rounds=`preset`, recovery=`600`, recovery_mode=`fixed`, settle_wait_ms=`250`, windows=`8`, drain_every=`1`, delay_event_to_smear=`0`

## Raw Results

```text
mode  scenario      run  baseline_us  recovery_ratio  stress_max_avg_us  stress_max_ratio  stress_tail_ratio  realized_mode  perf_class  probe_policy       line_count  callback_ewma_ms  reason_bits  extmark_fallback_calls  conceal_full_scan_calls
full  particles_on  1    1582.838     0.993           1582.838           1.000             0.921              full_full      full        exact              4000        0.0               0            0                       0
full  particles_on  2    1422.360     1.068           1571.076           1.105             1.105              full_full      full        exact              4000        0.0               0            0                       0
fast  particles_on  1    1537.523     1.004           1537.523           1.000             0.944              fast_fast      fast        deferred_extmarks  4000        0.0               0            0                       0
fast  particles_on  2    1436.080     1.022           1460.246           1.017             1.017              fast_fast      fast        deferred_extmarks  4000        0.1               0            0                       0
```

## Summary

```text
mode  scenario      avg_baseline_us  avg_recovery_ratio  avg_stress_max_ratio  avg_stress_tail_ratio  realized_mode  perf_class  probe_policy       line_count  avg_callback_ewma_ms  reason_bits
full  particles_on  1502.599         1.030               1.052                 1.013                  full_full      full        exact              4000        0.000                 0
fast  particles_on  1486.802         1.013               1.008                 0.980                  fast_fast      fast        deferred_extmarks  4000        0.050                 0
```

## Probe Cost Signals

```text
mode  scenario      avg_extmark_fallback_calls  avg_conceal_full_scan_calls
full  particles_on  0.00                        0.00
fast  particles_on  0.00                        0.00
```

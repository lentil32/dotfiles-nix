# Validation Counter Baseline

- Captured (UTC): `2026-04-16T07:47:51Z`
- Repo commit: `d425ed25eb26f12bdbdf84a5839b87f8f81579db`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.7`
- Command: `SMEAR_VALIDATION_REPORT_FILE=plugins/smear-cursor/perf/validation-counters-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/capture_validation_counters.sh`
- Config: repeats=`2`, scenarios=`large_line_count,extmark_heavy`, buffer_perf_mode=`full`, warmup=`300`, baseline=`600`, windows=`8`, drain_every=`1`

These rates use the delta between `PERF_VALIDATION phase=post_warmup` and
`PERF_VALIDATION phase=post_baseline` so they isolate the active baseline animation window.

## Raw Results

```text
scenario          run  baseline_elapsed_ms  baseline_avg_us  perf_class  probe_policy      buffer_metadata_reads  buffer_metadata_reads_per_s  current_buffer_changedtick_reads  current_buffer_changedtick_reads_per_s  editor_bounds_reads  editor_bounds_reads_per_s  command_row_reads  command_row_reads_per_s
large_line_count  1    736.444              1227.407         full        exact             0                      0.000                        0                                 0.000                                   0                    0.000                      0                  0.000
large_line_count  2    789.967              1316.612         full        exact             0                      0.000                        0                                 0.000                                   0                    0.000                      0                  0.000
extmark_heavy     1    777.825              1296.375         full        exact_compatible  0                      0.000                        0                                 0.000                                   0                    0.000                      0                  0.000
extmark_heavy     2    824.817              1374.695         full        exact_compatible  0                      0.000                        0                                 0.000                                   0                    0.000                      0                  0.000
```

## Summary

```text
scenario          avg_baseline_ms  avg_baseline_us  perf_class  probe_policy      avg_buffer_metadata_reads  avg_buffer_metadata_reads_per_s  avg_current_buffer_changedtick_reads  avg_current_buffer_changedtick_reads_per_s  avg_editor_bounds_reads  avg_editor_bounds_reads_per_s  avg_command_row_reads  avg_command_row_reads_per_s
large_line_count  763.206          1272.010         full        exact             0.0                        0.000                            0.0                                   0.000                                       0.0                      0.000                          0.0                    0.000
extmark_heavy     801.321          1335.535         full        exact_compatible  0.0                        0.000                            0.0                                   0.000                                       0.0                      0.000                          0.0                    0.000
```

# Validation Counter Baseline

- Captured (UTC): `2026-04-10T10:29:04Z`
- Repo commit: `5f01957cb6fa64a93ba15e5f98c6bb8a46c98c33`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.7`
- Command: `SMEAR_VALIDATION_REPORT_FILE=plugins/smear-cursor/perf/validation-counters-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/capture_validation_counters.sh`
- Config: repeats=`2`, scenarios=`large_line_count,extmark_heavy`, buffer_perf_mode=`full`, warmup=`300`, baseline=`600`, windows=`8`, drain_every=`1`

These rates use the delta between `PERF_VALIDATION phase=post_warmup` and
`PERF_VALIDATION phase=post_baseline` so they isolate the active baseline animation window.

## Raw Results

```text
scenario          run  baseline_elapsed_ms  baseline_avg_us  perf_class  probe_policy      buffer_metadata_reads  buffer_metadata_reads_per_s  current_buffer_changedtick_reads  current_buffer_changedtick_reads_per_s  editor_bounds_reads  editor_bounds_reads_per_s  command_row_reads  command_row_reads_per_s
large_line_count  1    741.002              1235.003         full        exact             0                      0.000                        0                                 0.000                                   0                    0.000                      0                  0.000
large_line_count  2    747.697              1246.161         full        exact             0                      0.000                        0                                 0.000                                   0                    0.000                      0                  0.000
extmark_heavy     1    746.942              1244.904         full        exact_compatible  0                      0.000                        0                                 0.000                                   0                    0.000                      0                  0.000
extmark_heavy     2    732.790              1221.317         full        exact_compatible  0                      0.000                        0                                 0.000                                   0                    0.000                      0                  0.000
```

## Summary

```text
scenario          avg_baseline_ms  avg_baseline_us  perf_class  probe_policy      avg_buffer_metadata_reads  avg_buffer_metadata_reads_per_s  avg_current_buffer_changedtick_reads  avg_current_buffer_changedtick_reads_per_s  avg_editor_bounds_reads  avg_editor_bounds_reads_per_s  avg_command_row_reads  avg_command_row_reads_per_s
large_line_count  744.350          1240.582         full        exact             0.0                        0.000                            0.0                                   0.000                                       0.0                      0.000                          0.0                    0.000
extmark_heavy     739.866          1233.111         full        exact_compatible  0.0                        0.000                            0.0                                   0.000                                       0.0                      0.000                          0.0                    0.000
```

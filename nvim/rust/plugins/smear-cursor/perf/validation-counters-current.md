# Validation Counter Baseline

- Captured (UTC): `2026-04-03T16:04:10Z`
- Repo commit: `8f0843c45d20d55dab22686dd92266a5d6c28649`
- Working tree: `dirty`
- Neovim: `NVIM v0.11.6`
- Command: `SMEAR_VALIDATION_REPORT_FILE=plugins/smear-cursor/perf/validation-counters-current.md /Users/lentil32/.nixpkgs/nvim/rust/plugins/smear-cursor/scripts/capture_validation_counters.sh`
- Config: repeats=`2`, scenarios=`large_line_count,extmark_heavy`, buffer_perf_mode=`full`, warmup=`300`, baseline=`600`, windows=`8`, drain_every=`1`

These rates use the delta between `PERF_VALIDATION phase=post_warmup` and
`PERF_VALIDATION phase=post_baseline` so they isolate the active baseline animation window.

## Raw Results

```text
scenario          run  baseline_elapsed_ms  baseline_avg_us  perf_class  probe_policy      buffer_metadata_reads  buffer_metadata_reads_per_s  current_buffer_changedtick_reads  current_buffer_changedtick_reads_per_s  editor_bounds_reads  editor_bounds_reads_per_s  command_row_reads  command_row_reads_per_s
large_line_count  1    735.200              1225.333         full        exact             0                      0.000                        652                               886.834                                 0                    0.000                      0                  0.000
large_line_count  2    737.440              1229.066         full        exact             0                      0.000                        616                               835.322                                 0                    0.000                      0                  0.000
extmark_heavy     1    742.097              1236.829         full        exact_compatible  0                      0.000                        625                               842.208                                 0                    0.000                      0                  0.000
extmark_heavy     2    745.356              1242.261         full        exact_compatible  0                      0.000                        639                               857.308                                 0                    0.000                      0                  0.000
```

## Summary

```text
scenario          avg_baseline_ms  avg_baseline_us  perf_class  probe_policy      avg_buffer_metadata_reads  avg_buffer_metadata_reads_per_s  avg_current_buffer_changedtick_reads  avg_current_buffer_changedtick_reads_per_s  avg_editor_bounds_reads  avg_editor_bounds_reads_per_s  avg_command_row_reads  avg_command_row_reads_per_s
large_line_count  736.320          1227.200         full        exact             0.0                        0.000                            634.0                                 861.078                                     0.0                      0.000                          0.0                    0.000
extmark_heavy     743.726          1239.545         full        exact_compatible  0.0                        0.000                            632.0                                 849.758                                     0.0                      0.000                          0.0                    0.000
```

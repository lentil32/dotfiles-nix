#!/usr/bin/env bash
set -euo pipefail

# Capture local validation-counter baselines from the window-switch harness.
#
# This focuses on phase deltas during the active baseline animation window so later
# perf patches can compare hot-path shell read frequency without mixing in warmup noise.
#
# Usage:
#   scripts/capture_validation_counters.sh
#
# Tunables:
#   SMEAR_VALIDATION_REPORT_FILE      (optional; write a markdown snapshot report)
#   SMEAR_VALIDATION_REPEATS          (default: 2)
#   SMEAR_VALIDATION_SCENARIOS        (default: large_line_count,extmark_heavy)
#   SMEAR_BUFFER_PERF_MODE            (default: full)
#   SMEAR_VALIDATION_WARMUP           (default: 300)
#   SMEAR_VALIDATION_BASELINE         (default: 600)
#   SMEAR_VALIDATION_WINDOWS          (default: 8)
#   SMEAR_VALIDATION_DRAIN_EVERY      (default: 1)

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd -- "${script_dir}/.." && pwd)"
workspace_dir="$(cd -- "${repo_dir}/../.." && pwd)"
driver_lua="${repo_dir}/scripts/perf_window_switch.lua"
perf_lib="${script_dir}/lib/perf_env.sh"

if [[ ! -f "${perf_lib}" ]]; then
  echo "missing perf helper: ${perf_lib}" >&2
  exit 1
fi
# shellcheck source=plugins/smear-cursor/scripts/lib/perf_env.sh
source "${perf_lib}"

repeats="${SMEAR_VALIDATION_REPEATS:-2}"
scenario_csv="${SMEAR_VALIDATION_SCENARIOS:-large_line_count,extmark_heavy}"
buffer_perf_mode="${SMEAR_BUFFER_PERF_MODE:-full}"
warmup_iterations="${SMEAR_VALIDATION_WARMUP:-300}"
baseline_iterations="${SMEAR_VALIDATION_BASELINE:-600}"
windows_count="${SMEAR_VALIDATION_WINDOWS:-8}"
drain_every="${SMEAR_VALIDATION_DRAIN_EVERY:-1}"
report_file="${SMEAR_VALIDATION_REPORT_FILE:-}"
export SMEAR_CARGO_FEATURES="${SMEAR_CARGO_FEATURES:-perf-counters}"

IFS=',' read -r -a scenarios <<< "${scenario_csv}"
if [[ "${#scenarios[@]}" -eq 0 ]]; then
  echo "SMEAR_VALIDATION_SCENARIOS must contain at least one preset" >&2
  exit 1
fi

artifact_dir="$(mktemp -d /tmp/smear_validation_counters.XXXXXX)"
results_tsv="${artifact_dir}/validation_counters.tsv"
raw_results_table="${artifact_dir}/validation_counters_raw.txt"
summary_table="${artifact_dir}/validation_counters_summary.txt"

smear_build_perf_report_tool "${workspace_dir}"

calc_delta() {
  local after="$1"
  local before="$2"
  awk -v after="${after}" -v before="${before}" 'BEGIN { printf "%.0f", after - before }'
}

calc_rate_per_s() {
  local count="$1"
  local elapsed_ms="$2"
  awk -v count="${count}" -v elapsed_ms="${elapsed_ms}" '
    BEGIN {
      if (elapsed_ms <= 0) {
        printf "0.000"
      } else {
        printf "%.3f", (count * 1000.0) / elapsed_ms
      }
    }
  '
}

run_once() {
  local scenario_name="$1"
  local repeat_index="$2"
  local log_file="${artifact_dir}/run_${scenario_name}_${repeat_index}.log"
  local baseline_line
  local diagnostics_line
  local warmup_validation_line
  local baseline_validation_line
  local baseline_elapsed_ms
  local baseline_avg_us
  local perf_class
  local probe_policy
  local warmup_buffer_metadata_reads
  local baseline_buffer_metadata_reads
  local delta_buffer_metadata_reads
  local buffer_metadata_reads_per_s
  local warmup_changedtick_reads
  local baseline_changedtick_reads
  local delta_changedtick_reads
  local changedtick_reads_per_s
  local warmup_editor_bounds_reads
  local baseline_editor_bounds_reads
  local delta_editor_bounds_reads
  local editor_bounds_reads_per_s
  local warmup_command_row_reads
  local baseline_command_row_reads
  local delta_command_row_reads
  local command_row_reads_per_s

  (
    cd "${repo_dir}"
    env \
      "SMEAR_CURSOR_RTP=${SMEAR_CURSOR_RTP}" \
      "SMEAR_CURSOR_CPATH=${SMEAR_CURSOR_CPATH}" \
      "SMEAR_SCENARIO_NAME=${scenario_name}" \
      "SMEAR_SCENARIO_PRESET=${scenario_name}" \
      "SMEAR_BUFFER_PERF_MODE=${buffer_perf_mode}" \
      "SMEAR_WINDOWS=${windows_count}" \
      "SMEAR_WARMUP_ITERATIONS=${warmup_iterations}" \
      "SMEAR_BASELINE_ITERATIONS=${baseline_iterations}" \
      "SMEAR_STRESS_ITERATIONS=1" \
      "SMEAR_STRESS_ROUNDS=1" \
      "SMEAR_RECOVERY_ITERATIONS=1" \
      "SMEAR_RECOVERY_MODE=fixed" \
      "SMEAR_SETTLE_WAIT_MS=0" \
      "SMEAR_MAX_RECOVERY_RATIO=100" \
      "SMEAR_MAX_STRESS_RATIO=100" \
      "SMEAR_DRAIN_EVERY=${drain_every}" \
      "${NVIM_BIN:-nvim}" --headless -u NONE -c "luafile ${driver_lua}"
  ) >"${log_file}" 2>&1

  IFS=$'\t' read -r \
    baseline_elapsed_ms \
    baseline_avg_us \
    perf_class \
    probe_policy \
    warmup_buffer_metadata_reads \
    baseline_buffer_metadata_reads \
    warmup_changedtick_reads \
    baseline_changedtick_reads \
    warmup_editor_bounds_reads \
    baseline_editor_bounds_reads \
    warmup_command_row_reads \
    baseline_command_row_reads \
    <<EOF
$(smear_perf_report_query "${workspace_dir}" "window-switch" "${log_file}" \
  "phases.baseline.elapsed_ms" \
  "phases.baseline.avg_us" \
  "diagnostics.post_baseline.perf_class" \
  "diagnostics.post_baseline.probe_policy" \
  "validation.post_warmup.bmr" \
  "validation.post_baseline.bmr" \
  "validation.post_warmup.cbtr" \
  "validation.post_baseline.cbtr" \
  "validation.post_warmup.ebr" \
  "validation.post_baseline.ebr" \
  "validation.post_warmup.crr" \
  "validation.post_baseline.crr")
EOF
  delta_buffer_metadata_reads="$(calc_delta "${baseline_buffer_metadata_reads}" "${warmup_buffer_metadata_reads}")"
  buffer_metadata_reads_per_s="$(calc_rate_per_s "${delta_buffer_metadata_reads}" "${baseline_elapsed_ms}")"

  delta_changedtick_reads="$(calc_delta "${baseline_changedtick_reads}" "${warmup_changedtick_reads}")"
  changedtick_reads_per_s="$(calc_rate_per_s "${delta_changedtick_reads}" "${baseline_elapsed_ms}")"

  delta_editor_bounds_reads="$(calc_delta "${baseline_editor_bounds_reads}" "${warmup_editor_bounds_reads}")"
  editor_bounds_reads_per_s="$(calc_rate_per_s "${delta_editor_bounds_reads}" "${baseline_elapsed_ms}")"

  delta_command_row_reads="$(calc_delta "${baseline_command_row_reads}" "${warmup_command_row_reads}")"
  command_row_reads_per_s="$(calc_rate_per_s "${delta_command_row_reads}" "${baseline_elapsed_ms}")"

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "${scenario_name}" \
    "${repeat_index}" \
    "${baseline_elapsed_ms}" \
    "${baseline_avg_us}" \
    "${perf_class}" \
    "${probe_policy}" \
    "${delta_buffer_metadata_reads}" \
    "${buffer_metadata_reads_per_s}" \
    "${delta_changedtick_reads}" \
    "${changedtick_reads_per_s}" \
    "${delta_editor_bounds_reads}" \
    "${editor_bounds_reads_per_s}" \
    "${delta_command_row_reads}" \
    "${command_row_reads_per_s}" \
    >>"${results_tsv}"

  printf '%-16s run=%s baseline_ms=%s buffer_metadata_reads/s=%s changedtick_reads/s=%s editor_bounds_reads/s=%s command_row_reads/s=%s\n' \
    "${scenario_name}" \
    "${repeat_index}" \
    "${baseline_elapsed_ms}" \
    "${buffer_metadata_reads_per_s}" \
    "${changedtick_reads_per_s}" \
    "${editor_bounds_reads_per_s}" \
    "${command_row_reads_per_s}"
}

render_raw_results() {
  column -t -s $'\t' "${results_tsv}"
}

render_summary_table() {
  {
    printf 'scenario\tavg_baseline_ms\tavg_baseline_us\tperf_class\tprobe_policy\tavg_buffer_metadata_reads\tavg_buffer_metadata_reads_per_s\tavg_current_buffer_changedtick_reads\tavg_current_buffer_changedtick_reads_per_s\tavg_editor_bounds_reads\tavg_editor_bounds_reads_per_s\tavg_command_row_reads\tavg_command_row_reads_per_s\n'
    for scenario_name in "${scenarios[@]}"; do
      awk -F '\t' -v scenario="${scenario_name}" '
        NR == 1 { next }
        $1 == scenario {
          baseline_ms_sum += $3
          baseline_us_sum += $4
          buffer_metadata_reads_sum += $7
          buffer_metadata_reads_per_s_sum += $8
          changedtick_reads_sum += $9
          changedtick_reads_per_s_sum += $10
          editor_bounds_reads_sum += $11
          editor_bounds_reads_per_s_sum += $12
          command_row_reads_sum += $13
          command_row_reads_per_s_sum += $14
          count += 1
          perf_class = $5
          probe_policy = $6
        }
        END {
          if (count > 0) {
            printf "%s\t%.3f\t%.3f\t%s\t%s\t%.1f\t%.3f\t%.1f\t%.3f\t%.1f\t%.3f\t%.1f\t%.3f\n",
              scenario,
              baseline_ms_sum / count,
              baseline_us_sum / count,
              perf_class,
              probe_policy,
              buffer_metadata_reads_sum / count,
              buffer_metadata_reads_per_s_sum / count,
              changedtick_reads_sum / count,
              changedtick_reads_per_s_sum / count,
              editor_bounds_reads_sum / count,
              editor_bounds_reads_per_s_sum / count,
              command_row_reads_sum / count,
              command_row_reads_per_s_sum / count
          }
        }
      ' "${results_tsv}"
    done
  } | column -t -s $'\t'
}

printf 'scenario\trun\tbaseline_elapsed_ms\tbaseline_avg_us\tperf_class\tprobe_policy\tbuffer_metadata_reads\tbuffer_metadata_reads_per_s\tcurrent_buffer_changedtick_reads\tcurrent_buffer_changedtick_reads_per_s\teditor_bounds_reads\teditor_bounds_reads_per_s\tcommand_row_reads\tcommand_row_reads_per_s\n' >"${results_tsv}"

echo "repo=${repo_dir}"
echo "artifacts=${artifact_dir}"
echo "scenarios=${scenario_csv}"
echo "buffer_perf_mode=${buffer_perf_mode}"
echo

smear_build_release "${repo_dir}"
if ! smear_export_runtime_paths "${repo_dir}" >/dev/null; then
  echo "failed to resolve runtime paths for ${repo_dir}" >&2
  exit 1
fi

for scenario_name in "${scenarios[@]}"; do
  for repeat_index in $(seq 1 "${repeats}"); do
    run_once "${scenario_name}" "${repeat_index}"
  done
done

render_raw_results >"${raw_results_table}"
render_summary_table >"${summary_table}"

echo
echo "== Raw Results =="
cat "${raw_results_table}"
echo
echo "== Summary =="
cat "${summary_table}"

if [[ -n "${report_file}" ]]; then
  mkdir -p "$(dirname -- "${report_file}")"
  cat >"${report_file}" <<EOF
# Validation Counter Baseline

- Captured (UTC): \`$(smear_report_capture_time_utc)\`
- Repo commit: \`$(smear_report_git_commit "${repo_dir}")\`
- Working tree: \`$(smear_report_git_state "${repo_dir}")\`
- Neovim: \`$(smear_report_nvim_version)\`
- Command: \`SMEAR_VALIDATION_REPORT_FILE=${report_file} ${repo_dir}/scripts/capture_validation_counters.sh\`
- Config: repeats=\`${repeats}\`, scenarios=\`${scenario_csv}\`, buffer_perf_mode=\`${buffer_perf_mode}\`, warmup=\`${warmup_iterations}\`, baseline=\`${baseline_iterations}\`, windows=\`${windows_count}\`, drain_every=\`${drain_every}\`

These rates use the delta between \`PERF_VALIDATION phase=post_warmup\` and
\`PERF_VALIDATION phase=post_baseline\` so they isolate the active baseline animation window.

## Raw Results

\`\`\`text
$(cat "${raw_results_table}")
\`\`\`

## Summary

\`\`\`text
$(cat "${summary_table}")
\`\`\`
EOF
  echo
  echo "wrote report to ${report_file}"
fi

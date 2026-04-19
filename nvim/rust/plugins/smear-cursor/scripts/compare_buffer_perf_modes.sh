#!/usr/bin/env bash
set -euo pipefail

# Compare adaptive buffer perf modes on the local working tree.
#
# This answers the question the commit-vs-commit wrappers cannot: for a single build, how does
# `buffer_perf_mode=auto` behave relative to forced `full`, `fast`, or `off` under the same
# workload preset?
#
# Usage:
#   scripts/compare_buffer_perf_modes.sh
#
# Tunables:
#   SMEAR_COMPARE_REPEATS              (default: 2)
#   SMEAR_COMPARE_MODES                (default: auto,full,fast)
#   SMEAR_COMPARE_SCENARIOS            (default: large_line_count,long_running_repetition,extmark_heavy,conceal_heavy)
#   SMEAR_COMPARE_WARMUP               (default: 300)
#   SMEAR_COMPARE_BASELINE             (default: 600)
#   SMEAR_COMPARE_STRESS               (default: 1200)
#   SMEAR_COMPARE_STRESS_ROUNDS        (optional override; otherwise presets keep their own rounds)
#   SMEAR_COMPARE_RECOVERY             (default: 600)
#   SMEAR_COMPARE_RECOVERY_MODE        (default: fixed)
#   SMEAR_COMPARE_SETTLE_WAIT_MS       (default: 250)
#   SMEAR_COMPARE_COLD_WAIT_TIMEOUT_MS (optional)
#   SMEAR_COMPARE_WINDOWS              (default: 8)
#   SMEAR_COMPARE_DRAIN_EVERY          (default: 1)
#   SMEAR_COMPARE_DELAY_EVENT_TO_SMEAR (default: 0)
#   SMEAR_COMPARE_MAX_RECOVERY_RATIO   (default: 100)
#   SMEAR_COMPARE_MAX_STRESS_RATIO     (default: 100)
#   SMEAR_COMPARE_REPORT_FILE          (optional; write a markdown snapshot report)

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

repeats="${SMEAR_COMPARE_REPEATS:-2}"
mode_csv="${SMEAR_COMPARE_MODES:-auto,full,fast}"
scenario_csv="${SMEAR_COMPARE_SCENARIOS:-large_line_count,long_running_repetition,extmark_heavy,conceal_heavy}"
IFS=',' read -r -a modes <<< "${mode_csv}"
IFS=',' read -r -a scenarios <<< "${scenario_csv}"
if [[ "${#modes[@]}" -eq 0 ]]; then
  echo "SMEAR_COMPARE_MODES must contain at least one mode" >&2
  exit 1
fi
if [[ "${#scenarios[@]}" -eq 0 ]]; then
  echo "SMEAR_COMPARE_SCENARIOS must contain at least one preset" >&2
  exit 1
fi

warmup_iterations="${SMEAR_COMPARE_WARMUP:-300}"
baseline_iterations="${SMEAR_COMPARE_BASELINE:-600}"
stress_iterations="${SMEAR_COMPARE_STRESS:-1200}"
stress_rounds="${SMEAR_COMPARE_STRESS_ROUNDS:-}"
recovery_iterations="${SMEAR_COMPARE_RECOVERY:-600}"
recovery_mode="${SMEAR_COMPARE_RECOVERY_MODE:-fixed}"
settle_wait_ms="${SMEAR_COMPARE_SETTLE_WAIT_MS:-250}"
cold_wait_timeout_ms="${SMEAR_COMPARE_COLD_WAIT_TIMEOUT_MS:-}"
windows_count="${SMEAR_COMPARE_WINDOWS:-8}"
drain_every="${SMEAR_COMPARE_DRAIN_EVERY:-1}"
delay_event_to_smear="${SMEAR_COMPARE_DELAY_EVENT_TO_SMEAR:-0}"
max_recovery_ratio="${SMEAR_COMPARE_MAX_RECOVERY_RATIO:-100}"
max_stress_ratio="${SMEAR_COMPARE_MAX_STRESS_RATIO:-100}"
report_file="${SMEAR_COMPARE_REPORT_FILE:-}"

artifact_dir="$(mktemp -d /tmp/smear_buffer_perf_modes.XXXXXX)"
results_tsv="${artifact_dir}/buffer_perf_modes.tsv"
raw_results_table="${artifact_dir}/buffer_perf_modes_raw.txt"
summary_table="${artifact_dir}/buffer_perf_modes_summary.txt"
adaptive_deltas_table="${artifact_dir}/buffer_perf_modes_adaptive_deltas.txt"
probe_cost_table="${artifact_dir}/buffer_perf_modes_probe_cost.txt"

run_mode() {
  local mode="$1"
  local scenario_name
  local repeat_index

  for scenario_name in "${scenarios[@]}"; do
    for repeat_index in $(seq 1 "${repeats}"); do
      run_once "${mode}" "${scenario_name}" "${repeat_index}"
    done
  done
}

printf 'mode\tscenario\trun\tbaseline_us\trecovery_ratio\tstress_max_avg_us\tstress_max_ratio\tstress_tail_ratio\trealized_mode\tperf_class\tprobe_policy\tline_count\tcallback_ewma_ms\treason_bits\textmark_fallback_calls\tconceal_full_scan_calls\n' >"${results_tsv}"

echo "repo=${repo_dir}"
echo "artifacts=${artifact_dir}"
echo "modes=${mode_csv}"
echo "scenarios=${scenario_csv}"
echo

smear_build_perf_report_tool "${workspace_dir}"
smear_build_release "${repo_dir}"
if ! smear_export_runtime_paths "${repo_dir}" >/dev/null; then
  echo "failed to resolve runtime paths for ${repo_dir}" >&2
  exit 1
fi

run_once() {
  local mode="$1"
  local scenario_name="$2"
  local repeat_index="$3"
  local log_file="${artifact_dir}/run_${mode}_${scenario_name}_${repeat_index}.log"
  local run_env=(
    "SMEAR_CURSOR_RTP=${repo_dir}"
    "SMEAR_CURSOR_CPATH=${SMEAR_CURSOR_CPATH}"
    "SMEAR_SCENARIO_NAME=${scenario_name}"
    "SMEAR_SCENARIO_PRESET=${scenario_name}"
    "SMEAR_BUFFER_PERF_MODE=${mode}"
    "SMEAR_WINDOWS=${windows_count}"
    "SMEAR_WARMUP_ITERATIONS=${warmup_iterations}"
    "SMEAR_BASELINE_ITERATIONS=${baseline_iterations}"
    "SMEAR_STRESS_ITERATIONS=${stress_iterations}"
    "SMEAR_RECOVERY_ITERATIONS=${recovery_iterations}"
    "SMEAR_RECOVERY_MODE=${recovery_mode}"
    "SMEAR_SETTLE_WAIT_MS=${settle_wait_ms}"
    "SMEAR_DRAIN_EVERY=${drain_every}"
    "SMEAR_DELAY_EVENT_TO_SMEAR=${delay_event_to_smear}"
    "SMEAR_MAX_RECOVERY_RATIO=${max_recovery_ratio}"
    "SMEAR_MAX_STRESS_RATIO=${max_stress_ratio}"
  )
  local summary_line
  local stress_line
  local diagnostics_line
  local baseline_us
  local recovery_ratio
  local stress_max_avg_us
  local stress_max_ratio
  local stress_tail_ratio
  local realized_mode
  local perf_class
  local probe_policy
  local line_count
  local callback_ewma_ms
  local reason_bits
  local extmark_fallback_calls
  local conceal_full_scan_calls

  if [[ -n "${stress_rounds}" ]]; then
    run_env+=("SMEAR_STRESS_ROUNDS=${stress_rounds}")
  fi
  if [[ -n "${cold_wait_timeout_ms}" ]]; then
    run_env+=("SMEAR_COLD_WAIT_TIMEOUT_MS=${cold_wait_timeout_ms}")
  fi

  (
    cd "${repo_dir}"
    env "${run_env[@]}" "${NVIM_BIN:-nvim}" --headless -u NONE -c "luafile ${driver_lua}"
  ) >"${log_file}" 2>&1

  IFS=$'\t' read -r \
    baseline_us \
    recovery_ratio \
    stress_max_avg_us \
    stress_max_ratio \
    stress_tail_ratio \
    realized_mode \
    perf_class \
    probe_policy \
    line_count \
    callback_ewma_ms \
    reason_bits \
    extmark_fallback_calls \
    conceal_full_scan_calls \
    <<EOF
$(smear_perf_report_query "${workspace_dir}" "window-switch" "${log_file}" \
  "summary.baseline_avg_us" \
  "summary.recovery_ratio" \
  "stress_summary.max_avg_us" \
  "stress_summary.max_ratio" \
  "stress_summary.tail_ratio" \
  "diagnostics.post_recovery.perf_effective_mode" \
  "diagnostics.post_recovery.perf_class" \
  "diagnostics.post_recovery.probe_policy" \
  "diagnostics.post_recovery.buffer_line_count" \
  "diagnostics.post_recovery.callback_ewma_ms" \
  "diagnostics.post_recovery.perf_reason_bits" \
  "diagnostics.post_recovery.cursor_color_extmark_fallback_calls" \
  "diagnostics.post_recovery.conceal_full_scan_calls")
EOF

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "${mode}" \
    "${scenario_name}" \
    "${repeat_index}" \
    "${baseline_us}" \
    "${recovery_ratio}" \
    "${stress_max_avg_us}" \
    "${stress_max_ratio}" \
    "${stress_tail_ratio}" \
    "${realized_mode}" \
    "${perf_class}" \
    "${probe_policy}" \
    "${line_count}" \
    "${callback_ewma_ms}" \
    "${reason_bits}" \
    "${extmark_fallback_calls}" \
    "${conceal_full_scan_calls}" \
    >>"${results_tsv}"

  printf '%-5s %-24s run=%s baseline=%8sus recovery_ratio=%s stress_max_ratio=%s class=%s probe=%s reasons=%s\n' \
    "${mode}" \
    "${scenario_name}" \
    "${repeat_index}" \
    "${baseline_us}" \
    "${recovery_ratio}" \
    "${stress_max_ratio}" \
    "${perf_class}" \
    "${probe_policy}" \
    "${reason_bits}"
}

render_raw_results() {
  column -t -s $'\t' "${results_tsv}"
}

render_summary_table() {
  {
    printf 'mode\tscenario\tavg_baseline_us\tavg_recovery_ratio\tavg_stress_max_ratio\tavg_stress_tail_ratio\trealized_mode\tperf_class\tprobe_policy\tline_count\tavg_callback_ewma_ms\treason_bits\n'
    for mode in "${modes[@]}"; do
      for scenario_name in "${scenarios[@]}"; do
        awk -F '\t' -v mode="${mode}" -v scenario="${scenario_name}" '
          NR == 1 { next }
          $1 == mode && $2 == scenario {
            baseline_sum += $4
            recovery_sum += $5
            stress_max_sum += $7
            stress_tail_sum += $8
            callback_sum += $13
            count += 1
            realized_mode = $9
            perf_class = $10
            probe_policy = $11
            line_count = $12
            reason_bits = $14
          }
          END {
            if (count > 0) {
              printf "%s\t%s\t%.3f\t%.3f\t%.3f\t%.3f\t%s\t%s\t%s\t%s\t%.3f\t%s\n",
                mode,
                scenario,
                baseline_sum / count,
                recovery_sum / count,
                stress_max_sum / count,
                stress_tail_sum / count,
                realized_mode,
                perf_class,
                probe_policy,
                line_count,
                callback_sum / count,
                reason_bits
            }
          }
        ' "${results_tsv}"
      done
    done
  } | column -t -s $'\t'
}

render_adaptive_deltas_table() {
  if [[ " ${modes[*]} " != *" auto "* ]]; then
    return
  fi

  {
    printf 'scenario\tauto_avg_baseline_us\tfull_avg_baseline_us\tfast_avg_baseline_us\tauto_vs_full_pct\tauto_vs_fast_pct\tauto_avg_recovery_ratio\tfull_avg_recovery_ratio\tfast_avg_recovery_ratio\tauto_avg_stress_max_ratio\tfull_avg_stress_max_ratio\tfast_avg_stress_max_ratio\tauto_class\tauto_probe\tauto_reason_bits\n'
    awk -F '\t' '
      NR == 1 { next }
      {
        key = $2 "|" $1
        baseline_sum[key] += $4
        recovery_sum[key] += $5
        stress_sum[key] += $7
        count[key] += 1
        perf_class[key] = $10
        probe_policy[key] = $11
        reason_bits[key] = $14
      }
      END {
        for (key in count) {
          split(key, parts, "|")
          scenarios[parts[1]] = 1
        }
        for (scenario in scenarios) {
          auto_key = scenario "|auto"
          if (count[auto_key] == 0) {
            continue
          }

          auto_baseline = baseline_sum[auto_key] / count[auto_key]
          auto_recovery = recovery_sum[auto_key] / count[auto_key]
          auto_stress = stress_sum[auto_key] / count[auto_key]
          full_key = scenario "|full"
          fast_key = scenario "|fast"

          full_baseline = count[full_key] > 0 ? baseline_sum[full_key] / count[full_key] : 0
          fast_baseline = count[fast_key] > 0 ? baseline_sum[fast_key] / count[fast_key] : 0
          full_recovery = count[full_key] > 0 ? recovery_sum[full_key] / count[full_key] : 0
          fast_recovery = count[fast_key] > 0 ? recovery_sum[fast_key] / count[fast_key] : 0
          full_stress = count[full_key] > 0 ? stress_sum[full_key] / count[full_key] : 0
          fast_stress = count[fast_key] > 0 ? stress_sum[fast_key] / count[fast_key] : 0

          if (count[full_key] > 0 && full_baseline > 0) {
            auto_vs_full = sprintf("%+.2f%%", (auto_baseline - full_baseline) / full_baseline * 100.0)
            full_baseline_text = sprintf("%.3f", full_baseline)
            full_recovery_text = sprintf("%.3f", full_recovery)
            full_stress_text = sprintf("%.3f", full_stress)
          } else {
            auto_vs_full = "na"
            full_baseline_text = "na"
            full_recovery_text = "na"
            full_stress_text = "na"
          }

          if (count[fast_key] > 0 && fast_baseline > 0) {
            auto_vs_fast = sprintf("%+.2f%%", (auto_baseline - fast_baseline) / fast_baseline * 100.0)
            fast_baseline_text = sprintf("%.3f", fast_baseline)
            fast_recovery_text = sprintf("%.3f", fast_recovery)
            fast_stress_text = sprintf("%.3f", fast_stress)
          } else {
            auto_vs_fast = "na"
            fast_baseline_text = "na"
            fast_recovery_text = "na"
            fast_stress_text = "na"
          }

          printf "%s\t%.3f\t%s\t%s\t%s\t%s\t%.3f\t%s\t%s\t%.3f\t%s\t%s\t%s\t%s\t%s\n",
            scenario,
            auto_baseline,
            full_baseline_text,
            fast_baseline_text,
            auto_vs_full,
            auto_vs_fast,
            auto_recovery,
            full_recovery_text,
            fast_recovery_text,
            auto_stress,
            full_stress_text,
            fast_stress_text,
            perf_class[auto_key],
            probe_policy[auto_key],
            reason_bits[auto_key]
        }
      }
    ' "${results_tsv}" | sort
  } | column -t -s $'\t'
}

render_probe_cost_table() {
  {
    printf 'mode\tscenario\tavg_extmark_fallback_calls\tavg_conceal_full_scan_calls\n'
    for mode in "${modes[@]}"; do
      for scenario_name in "${scenarios[@]}"; do
        awk -F '\t' -v mode="${mode}" -v scenario="${scenario_name}" '
          NR == 1 { next }
          $1 == mode && $2 == scenario {
            extmark_sum += $15
            conceal_sum += $16
            count += 1
          }
          END {
            if (count > 0) {
              printf "%s\t%s\t%.2f\t%.2f\n",
                mode,
                scenario,
                extmark_sum / count,
                conceal_sum / count
            }
          }
        ' "${results_tsv}"
      done
    done
  } | column -t -s $'\t'
}

write_report() {
  local output_file="$1"
  local git_commit
  local git_state
  local nvim_version
  local capture_time
  local command_line

  git_commit="$(smear_report_git_commit "${repo_dir}")"
  git_state="$(smear_report_git_state "${repo_dir}")"
  nvim_version="$(smear_report_nvim_version)"
  capture_time="$(smear_report_capture_time_utc)"
  command_line="SMEAR_COMPARE_REPORT_FILE=${output_file} ${repo_dir}/scripts/compare_buffer_perf_modes.sh"

  mkdir -p "$(dirname -- "${output_file}")"
  {
    printf '# Adaptive Buffer Perf Snapshot\n\n'
    printf -- '- Captured (UTC): %s\n' "${capture_time}"
    printf -- '- Repo commit: `%s`\n' "${git_commit}"
    printf -- '- Working tree: `%s`\n' "${git_state}"
    printf -- '- Neovim: `%s`\n' "${nvim_version}"
    printf -- '- Command: `%s`\n' "${command_line}"
    printf -- '- Config: repeats=`%s`, modes=`%s`, scenarios=`%s`, warmup=`%s`, baseline=`%s`, stress=`%s`, stress_rounds=`%s`, recovery=`%s`, recovery_mode=`%s`, settle_wait_ms=`%s`, windows=`%s`, drain_every=`%s`, delay_event_to_smear=`%s`\n' \
      "${repeats}" \
      "${mode_csv}" \
      "${scenario_csv}" \
      "${warmup_iterations}" \
      "${baseline_iterations}" \
      "${stress_iterations}" \
      "${stress_rounds:-preset}" \
      "${recovery_iterations}" \
      "${recovery_mode}" \
      "${settle_wait_ms}" \
      "${windows_count}" \
      "${drain_every}" \
      "${delay_event_to_smear}"
    printf '\n## Raw Results\n\n```text\n'
    cat "${raw_results_table}"
    printf '```\n\n## Summary\n\n```text\n'
    cat "${summary_table}"
    printf '```\n'
    if [[ -s "${adaptive_deltas_table}" ]]; then
      printf '\n## Adaptive Deltas\n\n```text\n'
      cat "${adaptive_deltas_table}"
      printf '```\n'
    fi
    printf '\n## Probe Cost Signals\n\n```text\n'
    cat "${probe_cost_table}"
    printf '```\n'
  } >"${output_file}"
}

for mode in "${modes[@]}"; do
  run_mode "${mode}"
done

echo
echo "== Raw Results =="
render_raw_results | tee "${raw_results_table}"

echo
echo "== Summary =="
render_summary_table | tee "${summary_table}"

if [[ " ${modes[*]} " == *" auto "* ]]; then
  echo
  echo "== Adaptive Deltas =="
  render_adaptive_deltas_table | tee "${adaptive_deltas_table}"
fi

echo
echo "== Probe Cost Signals =="
render_probe_cost_table | tee "${probe_cost_table}"

if [[ -n "${report_file}" ]]; then
  write_report "${report_file}"
  echo
  echo "report=${report_file}"
fi

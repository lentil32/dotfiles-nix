#!/usr/bin/env bash
set -euo pipefail

# Compare planner compile modes on the local working tree.
#
# This answers the planner benchmark checklist directly: how does forced
# reference full compile compare to the local-query path for the same release
# build and the same planner-heavy workload?
#
# Usage:
#   scripts/compare_planner_perf.sh
#
# Tunables:
#   SMEAR_COMPARE_REPEATS              (default: 2)
#   SMEAR_COMPARE_PLANNER_MODES        (default: reference,local_query)
#   SMEAR_COMPARE_SCENARIOS            (default: planner_heavy)
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
driver_lua="${repo_dir}/scripts/perf_window_switch.lua"
perf_lib="${script_dir}/lib/perf_env.sh"

if [[ ! -f "${perf_lib}" ]]; then
  echo "missing perf helper: ${perf_lib}" >&2
  exit 1
fi
# shellcheck source=plugins/smear-cursor/scripts/lib/perf_env.sh
source "${perf_lib}"

repeats="${SMEAR_COMPARE_REPEATS:-2}"
mode_csv="${SMEAR_COMPARE_PLANNER_MODES:-reference,local_query}"
scenario_csv="${SMEAR_COMPARE_SCENARIOS:-planner_heavy}"
IFS=',' read -r -a modes <<< "${mode_csv}"
IFS=',' read -r -a scenarios <<< "${scenario_csv}"
if [[ "${#modes[@]}" -eq 0 ]]; then
  echo "SMEAR_COMPARE_PLANNER_MODES must contain at least one mode" >&2
  exit 1
fi
if [[ "${#scenarios[@]}" -eq 0 ]]; then
  echo "SMEAR_COMPARE_SCENARIOS must contain at least one preset" >&2
  exit 1
fi

validate_mode() {
  local mode="$1"
  case "${mode}" in
    auto|reference|local_query) ;;
    *)
      echo "unsupported planner compile mode: ${mode}" >&2
      exit 1
      ;;
  esac
}

for mode in "${modes[@]}"; do
  validate_mode "${mode}"
done

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

artifact_dir="$(mktemp -d /tmp/smear_planner_compile_modes.XXXXXX)"
results_tsv="${artifact_dir}/planner_compile_modes.tsv"
raw_results_table="${artifact_dir}/planner_compile_modes_raw.txt"
summary_table="${artifact_dir}/planner_compile_modes_summary.txt"
worst_case_table="${artifact_dir}/planner_compile_modes_worst_case.txt"
planner_telemetry_table="${artifact_dir}/planner_compile_modes_telemetry.txt"
compile_deltas_table="${artifact_dir}/planner_compile_modes_deltas.txt"

extract_field() {
  local line="$1"
  local field="$2"

  printf '%s\n' "${line}" | sed -nE "s/.*${field}=([^ ]+).*/\\1/p"
}

build_release() {
  (
    cd "${repo_dir}"
    cargo build --release >/dev/null
  )
}

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

printf 'mode\tscenario\trun\tbaseline_us\trecovery_ratio\tstress_max_avg_us\tstress_max_ratio\tstress_tail_ratio\tperf_class\tline_count\tplanner_bucket_maps_scanned\tplanner_bucket_cells_scanned\tplanner_local_query_envelope_area_cells\tplanner_local_query_cells\tplanner_compiled_query_cells\tplanner_candidate_query_cells\tplanner_compiled_cells_emitted\tplanner_candidate_cells_built\tplanner_reference_compiles\tplanner_local_query_compiles\trealized_path\n' >"${results_tsv}"

echo "repo=${repo_dir}"
echo "artifacts=${artifact_dir}"
echo "modes=${mode_csv}"
echo "scenarios=${scenario_csv}"
echo

build_release
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
    "SMEAR_PLANNER_COMPILE_MODE=${mode}"
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
  local perf_class
  local line_count
  local planner_bucket_maps_scanned
  local planner_bucket_cells_scanned
  local planner_local_query_envelope_area_cells
  local planner_local_query_cells
  local planner_compiled_query_cells
  local planner_candidate_query_cells
  local planner_compiled_cells_emitted
  local planner_candidate_cells_built
  local planner_reference_compiles
  local planner_local_query_compiles
  local realized_path

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

  summary_line="$(grep 'PERF_SUMMARY' "${log_file}" | tail -n 1)"
  stress_line="$(grep 'PERF_STRESS_SUMMARY' "${log_file}" | tail -n 1)"
  diagnostics_line="$(grep 'PERF_DIAGNOSTICS phase=post_recovery ' "${log_file}" | tail -n 1)"
  if [[ -z "${summary_line}" || -z "${stress_line}" || -z "${diagnostics_line}" ]]; then
    echo "missing perf summary fields in ${log_file}" >&2
    exit 1
  fi

  baseline_us="$(extract_field "${summary_line}" "baseline_avg_us")"
  recovery_ratio="$(extract_field "${summary_line}" "recovery_ratio")"
  stress_max_avg_us="$(extract_field "${stress_line}" "max_avg_us")"
  stress_max_ratio="$(extract_field "${stress_line}" "max_ratio")"
  stress_tail_ratio="$(extract_field "${stress_line}" "tail_ratio")"
  perf_class="$(extract_field "${diagnostics_line}" "perf_class")"
  line_count="$(extract_field "${diagnostics_line}" "buffer_line_count")"
  planner_bucket_maps_scanned="$(extract_field "${diagnostics_line}" "planner_bms")"
  planner_bucket_cells_scanned="$(extract_field "${diagnostics_line}" "planner_bcs")"
  planner_local_query_envelope_area_cells="$(extract_field "${diagnostics_line}" "planner_lqea")"
  planner_local_query_cells="$(extract_field "${diagnostics_line}" "planner_local_query_cells")"
  planner_compiled_query_cells="$(extract_field "${diagnostics_line}" "planner_compq")"
  planner_candidate_query_cells="$(extract_field "${diagnostics_line}" "planner_candq")"
  planner_compiled_cells_emitted="$(extract_field "${diagnostics_line}" "planner_compiled_cells_emitted")"
  planner_candidate_cells_built="$(extract_field "${diagnostics_line}" "planner_candidate_cells_built")"
  planner_reference_compiles="$(extract_field "${diagnostics_line}" "planner_rc")"
  planner_local_query_compiles="$(extract_field "${diagnostics_line}" "planner_lqc")"
  if [[ "${planner_reference_compiles}" != "0" && "${planner_local_query_compiles}" == "0" ]]; then
    realized_path="reference"
  elif [[ "${planner_reference_compiles}" == "0" && "${planner_local_query_compiles}" != "0" ]]; then
    realized_path="local_query"
  else
    echo "planner compile counters were ambiguous in ${log_file}" >&2
    exit 1
  fi

  case "${mode}" in
    reference)
      if [[ "${realized_path}" != "reference" ]]; then
        echo "forced reference compile unexpectedly used local-query path in ${log_file}" >&2
        exit 1
      fi
      ;;
    local_query)
      if [[ "${realized_path}" != "local_query" ]]; then
        echo "forced local_query compile did not produce local-query telemetry in ${log_file}" >&2
        exit 1
      fi
      ;;
  esac

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "${mode}" \
    "${scenario_name}" \
    "${repeat_index}" \
    "${baseline_us}" \
    "${recovery_ratio}" \
    "${stress_max_avg_us}" \
    "${stress_max_ratio}" \
    "${stress_tail_ratio}" \
    "${perf_class}" \
    "${line_count}" \
    "${planner_bucket_maps_scanned}" \
    "${planner_bucket_cells_scanned}" \
    "${planner_local_query_envelope_area_cells}" \
    "${planner_local_query_cells}" \
    "${planner_compiled_query_cells}" \
    "${planner_candidate_query_cells}" \
    "${planner_compiled_cells_emitted}" \
    "${planner_candidate_cells_built}" \
    "${planner_reference_compiles}" \
    "${planner_local_query_compiles}" \
    "${realized_path}" \
    >>"${results_tsv}"

  printf '%-11s %-16s run=%s baseline=%8sus recovery_ratio=%s stress_max_ratio=%s planner_maps=%s planner_cells=%s envelope_area=%s local_query=%s compiled_query=%s candidate_query=%s compiled=%s candidates=%s reference_compiles=%s local_query_compiles=%s path=%s\n' \
    "${mode}" \
    "${scenario_name}" \
    "${repeat_index}" \
    "${baseline_us}" \
    "${recovery_ratio}" \
    "${stress_max_ratio}" \
    "${planner_bucket_maps_scanned}" \
    "${planner_bucket_cells_scanned}" \
    "${planner_local_query_envelope_area_cells}" \
    "${planner_local_query_cells}" \
    "${planner_compiled_query_cells}" \
    "${planner_candidate_query_cells}" \
    "${planner_compiled_cells_emitted}" \
    "${planner_candidate_cells_built}" \
    "${planner_reference_compiles}" \
    "${planner_local_query_compiles}" \
    "${realized_path}"
}

render_raw_results() {
  column -t -s $'\t' "${results_tsv}"
}

render_summary_table() {
  {
    printf 'mode\tscenario\tavg_baseline_us\tavg_recovery_ratio\tavg_stress_max_ratio\tavg_stress_tail_ratio\tperf_class\tline_count\trealized_path\n'
    for mode in "${modes[@]}"; do
      for scenario_name in "${scenarios[@]}"; do
        awk -F '\t' -v mode="${mode}" -v scenario="${scenario_name}" '
          NR == 1 { next }
          $1 == mode && $2 == scenario {
            baseline_sum += $4
            recovery_sum += $5
            stress_max_sum += $7
            stress_tail_sum += $8
            count += 1
            perf_class = $9
            line_count = $10
            realized_path = $21
          }
          END {
            if (count > 0) {
              printf "%s\t%s\t%.3f\t%.3f\t%.3f\t%.3f\t%s\t%s\t%s\n",
                mode,
                scenario,
                baseline_sum / count,
                recovery_sum / count,
                stress_max_sum / count,
                stress_tail_sum / count,
                perf_class,
                line_count,
                realized_path
            }
          }
        ' "${results_tsv}"
      done
    done
  } | column -t -s $'\t'
}

render_worst_case_table() {
  {
    printf 'mode\tscenario\tworst_baseline_us\tworst_recovery_ratio\tworst_stress_max_avg_us\tworst_stress_max_ratio\tworst_stress_tail_ratio\trealized_path\n'
    for mode in "${modes[@]}"; do
      for scenario_name in "${scenarios[@]}"; do
        awk -F '\t' -v mode="${mode}" -v scenario="${scenario_name}" '
          NR == 1 { next }
          $1 == mode && $2 == scenario {
            if ($4 > worst_baseline_us) {
              worst_baseline_us = $4
            }
            if ($5 > worst_recovery_ratio) {
              worst_recovery_ratio = $5
            }
            if ($6 > worst_stress_max_avg_us) {
              worst_stress_max_avg_us = $6
            }
            if ($7 > worst_stress_max_ratio) {
              worst_stress_max_ratio = $7
            }
            if ($8 > worst_stress_tail_ratio) {
              worst_stress_tail_ratio = $8
            }
            count += 1
            realized_path = $21
          }
          END {
            if (count > 0) {
              printf "%s\t%s\t%.3f\t%.3f\t%.3f\t%.3f\t%.3f\t%s\n",
                mode,
                scenario,
                worst_baseline_us,
                worst_recovery_ratio,
                worst_stress_max_avg_us,
                worst_stress_max_ratio,
                worst_stress_tail_ratio,
                realized_path
            }
          }
        ' "${results_tsv}"
      done
    done
  } | column -t -s $'\t'
}

render_planner_telemetry_table() {
  {
    printf 'mode\tscenario\tavg_planner_bucket_maps_scanned\tmax_planner_bucket_maps_scanned\tavg_planner_bucket_cells_scanned\tmax_planner_bucket_cells_scanned\tavg_planner_local_query_envelope_area_cells\tmax_planner_local_query_envelope_area_cells\tavg_planner_local_query_cells\tmax_planner_local_query_cells\tavg_planner_compiled_query_cells\tmax_planner_compiled_query_cells\tavg_planner_candidate_query_cells\tmax_planner_candidate_query_cells\tavg_planner_compiled_cells_emitted\tmax_planner_compiled_cells_emitted\tavg_planner_candidate_cells_built\tmax_planner_candidate_cells_built\tavg_planner_reference_compiles\tavg_planner_local_query_compiles\trealized_path\n'
    for mode in "${modes[@]}"; do
      for scenario_name in "${scenarios[@]}"; do
        awk -F '\t' -v mode="${mode}" -v scenario="${scenario_name}" '
          NR == 1 { next }
          $1 == mode && $2 == scenario {
            bucket_maps_sum += $11
            bucket_cells_sum += $12
            envelope_area_sum += $13
            local_query_sum += $14
            compiled_query_sum += $15
            candidate_query_sum += $16
            compiled_sum += $17
            candidate_sum += $18
            reference_compile_sum += $19
            local_query_compile_sum += $20
            if ($11 > bucket_maps_max) {
              bucket_maps_max = $11
            }
            if ($12 > bucket_cells_max) {
              bucket_cells_max = $12
            }
            if ($13 > envelope_area_max) {
              envelope_area_max = $13
            }
            if ($14 > local_query_max) {
              local_query_max = $14
            }
            if ($15 > compiled_query_max) {
              compiled_query_max = $15
            }
            if ($16 > candidate_query_max) {
              candidate_query_max = $16
            }
            if ($17 > compiled_max) {
              compiled_max = $17
            }
            if ($18 > candidate_max) {
              candidate_max = $18
            }
            count += 1
            realized_path = $21
          }
          END {
            if (count > 0) {
              printf "%s\t%s\t%.2f\t%.0f\t%.2f\t%.0f\t%.2f\t%.0f\t%.2f\t%.0f\t%.2f\t%.0f\t%.2f\t%.0f\t%.2f\t%.0f\t%.2f\t%.0f\t%.2f\t%.2f\t%s\n",
                mode,
                scenario,
                bucket_maps_sum / count,
                bucket_maps_max,
                bucket_cells_sum / count,
                bucket_cells_max,
                envelope_area_sum / count,
                envelope_area_max,
                local_query_sum / count,
                local_query_max,
                compiled_query_sum / count,
                compiled_query_max,
                candidate_query_sum / count,
                candidate_query_max,
                compiled_sum / count,
                compiled_max,
                candidate_sum / count,
                candidate_max,
                reference_compile_sum / count,
                local_query_compile_sum / count,
                realized_path
            }
          }
        ' "${results_tsv}"
      done
    done
  } | column -t -s $'\t'
}

render_compile_deltas_table() {
  if [[ " ${modes[*]} " != *" reference "* || " ${modes[*]} " != *" local_query "* ]]; then
    return
  fi

  {
    printf 'scenario\treference_avg_baseline_us\tlocal_query_avg_baseline_us\tlocal_query_vs_reference_pct\treference_avg_recovery_ratio\tlocal_query_avg_recovery_ratio\treference_avg_stress_max_ratio\tlocal_query_avg_stress_max_ratio\treference_avg_planner_bucket_maps_scanned\tlocal_query_avg_planner_bucket_maps_scanned\treference_avg_planner_bucket_cells_scanned\tlocal_query_avg_planner_bucket_cells_scanned\treference_avg_planner_compiled_cells_emitted\tlocal_query_avg_planner_compiled_cells_emitted\treference_avg_planner_candidate_cells_built\tlocal_query_avg_planner_candidate_cells_built\treference_avg_planner_reference_compiles\tlocal_query_avg_planner_local_query_compiles\n'
    awk -F '\t' '
      NR == 1 { next }
      {
        key = $2 "|" $1
        baseline_sum[key] += $4
        recovery_sum[key] += $5
        stress_sum[key] += $7
        bucket_maps_sum[key] += $11
        bucket_cells_sum[key] += $12
        compiled_sum[key] += $17
        candidate_sum[key] += $18
        reference_compile_sum[key] += $19
        local_query_compile_sum[key] += $20
        count[key] += 1
      }
      END {
        for (key in count) {
          split(key, parts, "|")
          scenarios[parts[1]] = 1
        }
        for (scenario in scenarios) {
          reference_key = scenario "|reference"
          local_query_key = scenario "|local_query"
          if (count[reference_key] == 0 || count[local_query_key] == 0) {
            continue
          }

          reference_baseline = baseline_sum[reference_key] / count[reference_key]
          local_query_baseline = baseline_sum[local_query_key] / count[local_query_key]
          local_query_vs_reference = (local_query_baseline - reference_baseline) / reference_baseline * 100.0

          printf "%s\t%.3f\t%.3f\t%+.2f%%\t%.3f\t%.3f\t%.3f\t%.3f\t%.2f\t%.2f\t%.2f\t%.2f\t%.2f\t%.2f\t%.2f\t%.2f\t%.2f\t%.2f\n",
            scenario,
            reference_baseline,
            local_query_baseline,
            local_query_vs_reference,
            recovery_sum[reference_key] / count[reference_key],
            recovery_sum[local_query_key] / count[local_query_key],
            stress_sum[reference_key] / count[reference_key],
            stress_sum[local_query_key] / count[local_query_key],
            bucket_maps_sum[reference_key] / count[reference_key],
            bucket_maps_sum[local_query_key] / count[local_query_key],
            bucket_cells_sum[reference_key] / count[reference_key],
            bucket_cells_sum[local_query_key] / count[local_query_key],
            compiled_sum[reference_key] / count[reference_key],
            compiled_sum[local_query_key] / count[local_query_key],
            candidate_sum[reference_key] / count[reference_key],
            candidate_sum[local_query_key] / count[local_query_key],
            reference_compile_sum[reference_key] / count[reference_key],
            local_query_compile_sum[local_query_key] / count[local_query_key]
        }
      }
    ' "${results_tsv}" | sort
  } | column -t -s $'\t'
}

write_report() {
  local output_file="$1"
  local git_commit
  local git_state
  local nvim_version
  local capture_time
  local command_line

  git_commit="$(git -C "${repo_dir}" rev-parse HEAD 2>/dev/null || printf 'unknown\n')"
  if [[ -n "$(git -C "${repo_dir}" status --short 2>/dev/null)" ]]; then
    git_state="dirty"
  else
    git_state="clean"
  fi
  nvim_version="$("${NVIM_BIN:-nvim}" --version | sed -n '1p')"
  capture_time="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  command_line="SMEAR_COMPARE_REPORT_FILE=${output_file} ${repo_dir}/scripts/compare_planner_perf.sh"

  mkdir -p "$(dirname -- "${output_file}")"
  {
    printf '# Planner Compile Perf Snapshot\n\n'
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
    printf '```\n\n## Worst-Case Spikes\n\n```text\n'
    cat "${worst_case_table}"
    printf '```\n\n## Planner Telemetry\n\n```text\n'
    cat "${planner_telemetry_table}"
    printf '```\n'
    if [[ -s "${compile_deltas_table}" ]]; then
      printf '\n## Compile Deltas\n\n```text\n'
      cat "${compile_deltas_table}"
      printf '```\n'
    fi
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

echo
echo "== Worst-Case Spikes =="
render_worst_case_table | tee "${worst_case_table}"

echo
echo "== Planner Telemetry =="
render_planner_telemetry_table | tee "${planner_telemetry_table}"

if [[ " ${modes[*]} " == *" reference "* && " ${modes[*]} " == *" local_query "* ]]; then
  echo
  echo "== Compile Deltas =="
  render_compile_deltas_table | tee "${compile_deltas_table}"
fi

if [[ -n "${report_file}" ]]; then
  write_report "${report_file}"
  echo
  echo "report=${report_file}"
fi

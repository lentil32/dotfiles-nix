#!/usr/bin/env bash
set -euo pipefail

# Compare perf_window_switch preset scenarios for the local working tree against a base git ref.
#
# Usage:
#   scripts/compare_window_switch_scenarios.sh
#   scripts/compare_window_switch_scenarios.sh HEAD~1
#
# Tunables:
#   SMEAR_COMPARE_REPEATS              (default: 2)
#   SMEAR_COMPARE_SCENARIOS            (default: large_line_count,long_running_repetition,planner_heavy,extmark_heavy,conceal_heavy,particles_off,particles_on)
#   SMEAR_COMPARE_WARMUP               (default: 300)
#   SMEAR_COMPARE_BASELINE             (default: 600)
#   SMEAR_COMPARE_STRESS               (default: 1200)
#   SMEAR_COMPARE_STRESS_ROUNDS        (optional override; by default each preset keeps its own rounds)
#   SMEAR_COMPARE_RECOVERY             (default: 600)
#   SMEAR_COMPARE_RECOVERY_MODE        (default: fixed)
#   SMEAR_COMPARE_SETTLE_WAIT_MS       (default: 250)
#   SMEAR_COMPARE_COLD_WAIT_TIMEOUT_MS (optional)
#   SMEAR_COMPARE_WINDOWS              (default: 8)
#   SMEAR_COMPARE_DRAIN_EVERY          (default: 1)
#   SMEAR_COMPARE_DELAY_EVENT_TO_SMEAR (default: 0)
#   SMEAR_COMPARE_MAX_RECOVERY_RATIO   (default: 100)
#   SMEAR_COMPARE_MAX_STRESS_RATIO     (default: 100)
#   SMEAR_COMPARE_LOCAL_OVERRIDES      (optional env words, e.g. "SMEAR_MAX_KEPT_WINDOWS=64")
#   SMEAR_COMPARE_BASE_OVERRIDES       (optional env words, e.g. "SMEAR_MAX_KEPT_WINDOWS=384")
#   SMEAR_COMPARE_REPORT_COMMAND       (optional; override command text written into the report)
#   SMEAR_COMPARE_REPORT_FILE          (optional; write a markdown snapshot report)

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
rust_repo_dir="$(cd -- "${script_dir}/../../.." && pwd)"
repo_root="$(cd -- "${rust_repo_dir}/../.." && pwd)"
driver_lua="${rust_repo_dir}/plugins/smear-cursor/scripts/perf_window_switch.lua"
perf_lib="${script_dir}/lib/perf_env.sh"

if [[ ! -f "${perf_lib}" ]]; then
  echo "missing perf helper: ${perf_lib}" >&2
  exit 1
fi
# shellcheck source=plugins/smear-cursor/scripts/lib/perf_env.sh
source "${perf_lib}"

base_ref="${1:-HEAD}"
repeats="${SMEAR_COMPARE_REPEATS:-2}"
scenario_csv="${SMEAR_COMPARE_SCENARIOS:-large_line_count,long_running_repetition,planner_heavy,extmark_heavy,conceal_heavy,particles_off,particles_on}"
IFS=',' read -r -a scenarios <<< "${scenario_csv}"
if [[ "${#scenarios[@]}" -eq 0 ]]; then
  echo "SMEAR_COMPARE_SCENARIOS must contain at least one preset" >&2
  exit 1
fi

has_scenario() {
  local needle="$1"
  local scenario_name
  for scenario_name in "${scenarios[@]}"; do
    if [[ "${scenario_name}" == "${needle}" ]]; then
      return 0
    fi
  done
  return 1
}

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
report_command="${SMEAR_COMPARE_REPORT_COMMAND:-}"
local_overrides_raw="${SMEAR_COMPARE_LOCAL_OVERRIDES:-}"
base_overrides_raw="${SMEAR_COMPARE_BASE_OVERRIDES:-}"

local_overrides=()
base_overrides=()
if [[ -n "${local_overrides_raw}" ]]; then
  read -r -a local_overrides <<< "${local_overrides_raw}"
fi
if [[ -n "${base_overrides_raw}" ]]; then
  read -r -a base_overrides <<< "${base_overrides_raw}"
fi

IFS=$'\t' read -r worktree_dir artifact_dir <<EOF
$(smear_compare_prepare_worktree "${repo_root}" "${base_ref}" "smear_window_switch_compare" "smear_window_switch_compare_artifacts")
EOF
results_tsv="${artifact_dir}/window_switch_compare_results.tsv"
raw_results_table="${artifact_dir}/window_switch_compare_raw.txt"
summary_table="${artifact_dir}/window_switch_compare_summary.txt"
worst_case_table="${artifact_dir}/window_switch_compare_worst_case.txt"
planner_telemetry_table="${artifact_dir}/window_switch_compare_planner_telemetry.txt"
planner_validation_table="${artifact_dir}/window_switch_compare_planner_validation.txt"
ingress_fast_path_table="${artifact_dir}/window_switch_compare_ingress_fast_path.txt"
pool_retention_table="${artifact_dir}/window_switch_compare_pool_retention.txt"
pool_peak_pressure_table="${artifact_dir}/window_switch_compare_pool_peak_pressure.txt"
particle_isolation_table="${artifact_dir}/window_switch_compare_particle_isolation.txt"
delta_table="${artifact_dir}/window_switch_compare_delta.txt"

cleanup() {
  smear_compare_remove_worktree "${repo_root}" "${worktree_dir}"
}
trap cleanup EXIT

run_once() {
  local side_label="$1"
  local side_root="$2"
  local scenario_name="$3"
  local repeat_index="$4"
  local plugin_dir
  local log_file="${artifact_dir}/run_${side_label}_${scenario_name}_${repeat_index}.log"
  local smear_cursor_cpath
  local run_env
  local summary_line
  local stress_line
  local diagnostics_line
  local validation_line
  local baseline_us
  local recovery_ratio
  local stress_max_avg_us
  local stress_max_ratio
  local stress_tail_ratio
  local perf_class
  local line_count
  local extmark_fallback_calls
  local conceal_full_scan_calls
  local planner_bucket_maps_scanned
  local planner_bucket_cells_scanned
  local planner_local_query_envelope_area_cells
  local planner_local_query_cells
  local planner_compiled_query_cells
  local planner_candidate_query_cells
  local planner_compiled_cells_emitted
  local planner_candidate_cells_built
  local projection_reuse_hits
  local projection_reuse_misses
  local planner_cache_hits
  local planner_cache_misses
  local planner_validation_compiled_cells_emitted
  local planner_validation_candidate_cells_built
  local pool_total_windows
  local pool_cached_budget
  local max_kept_windows
  local pool_peak_requested_capacity
  local pool_capacity_cap_hits
  local win_enter_dropped
  local win_enter_continued
  local win_scrolled_dropped
  local win_scrolled_continued
  local buf_enter_dropped
  local buf_enter_continued
  local side_overrides=()

  plugin_dir="$(smear_compare_plugin_dir "${side_root}")"
  smear_cursor_cpath="$(smear_compare_release_cpath "${plugin_dir}")"
  if [[ -z "${smear_cursor_cpath}" ]]; then
    echo "failed to resolve release cpath for ${side_label}" >&2
    exit 1
  fi

  run_env=(
    "SMEAR_CURSOR_RTP=${plugin_dir}"
    "SMEAR_CURSOR_CPATH=${smear_cursor_cpath}"
    "SMEAR_SCENARIO_NAME=${scenario_name}"
    "SMEAR_SCENARIO_PRESET=${scenario_name}"
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
  if [[ -n "${stress_rounds}" ]]; then
    run_env+=("SMEAR_STRESS_ROUNDS=${stress_rounds}")
  fi
  if [[ -n "${cold_wait_timeout_ms}" ]]; then
    run_env+=("SMEAR_COLD_WAIT_TIMEOUT_MS=${cold_wait_timeout_ms}")
  fi
  if [[ "${side_label}" == "local" ]]; then
    side_overrides=("${local_overrides[@]}")
  else
    side_overrides=("${base_overrides[@]}")
  fi

  (
    cd "${plugin_dir}"
    env "${side_overrides[@]}" "${run_env[@]}" "${NVIM_BIN:-nvim}" --headless -u NONE -c "luafile ${driver_lua}"
  ) >"${log_file}" 2>&1

  IFS=$'\t' read -r \
    baseline_us \
    recovery_ratio \
    stress_max_avg_us \
    stress_max_ratio \
    stress_tail_ratio \
    perf_class \
    line_count \
    extmark_fallback_calls \
    conceal_full_scan_calls \
    planner_bucket_maps_scanned \
    planner_bucket_cells_scanned \
    planner_local_query_envelope_area_cells \
    planner_local_query_cells \
    planner_compiled_query_cells \
    planner_candidate_query_cells \
    planner_compiled_cells_emitted \
    planner_candidate_cells_built \
    projection_reuse_hits \
    projection_reuse_misses \
    planner_cache_hits \
    planner_cache_misses \
    planner_validation_compiled_cells_emitted \
    planner_validation_candidate_cells_built \
    pool_total_windows \
    pool_cached_budget \
    max_kept_windows \
    pool_peak_requested_capacity \
    pool_capacity_cap_hits \
    win_enter_dropped \
    win_enter_continued \
    win_scrolled_dropped \
    win_scrolled_continued \
    buf_enter_dropped \
    buf_enter_continued \
    <<EOF
$(smear_perf_report_query "${rust_repo_dir}" "window-switch" "${log_file}" \
  "summary.baseline_avg_us" \
  "summary.recovery_ratio" \
  "stress_summary.max_avg_us" \
  "stress_summary.max_ratio" \
  "stress_summary.tail_ratio" \
  "diagnostics.post_recovery.perf_class" \
  "diagnostics.post_recovery.buffer_line_count" \
  "diagnostics.post_recovery.cursor_color_extmark_fallback_calls" \
  "diagnostics.post_recovery.conceal_full_scan_calls" \
  "diagnostics.post_recovery.planner_bms" \
  "diagnostics.post_recovery.planner_bcs" \
  "diagnostics.post_recovery.planner_lqea" \
  "diagnostics.post_recovery.planner_local_query_cells" \
  "diagnostics.post_recovery.planner_compq" \
  "diagnostics.post_recovery.planner_candq" \
  "diagnostics.post_recovery.planner_compiled_cells_emitted" \
  "diagnostics.post_recovery.planner_candidate_cells_built" \
  "validation.post_recovery.prh=na" \
  "validation.post_recovery.prm=na" \
  "validation.post_recovery.pch=na" \
  "validation.post_recovery.pcm=na" \
  "validation.post_recovery.pce=na" \
  "validation.post_recovery.pcb=na" \
  "diagnostics.post_recovery.pool_total_windows" \
  "diagnostics.post_recovery.pool_cached_budget=na" \
  "diagnostics.post_recovery.max_kept_windows=na" \
  "diagnostics.post_recovery.pool_peak_requested=na" \
  "diagnostics.post_recovery.pool_cap_hits=na" \
  "validation.post_recovery.wed=na" \
  "validation.post_recovery.wec=na" \
  "validation.post_recovery.wsd=na" \
  "validation.post_recovery.wsc=na" \
  "validation.post_recovery.bed=na" \
  "validation.post_recovery.bec=na")
EOF

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "${side_label}" \
    "${scenario_name}" \
    "${repeat_index}" \
    "${baseline_us}" \
    "${recovery_ratio}" \
    "${stress_max_avg_us}" \
    "${stress_max_ratio}" \
    "${stress_tail_ratio}" \
    "${perf_class}" \
    "${line_count}" \
    "${extmark_fallback_calls}" \
    "${conceal_full_scan_calls}" \
    "${planner_bucket_maps_scanned}" \
    "${planner_bucket_cells_scanned}" \
    "${planner_local_query_envelope_area_cells}" \
    "${planner_local_query_cells}" \
    "${planner_compiled_query_cells}" \
    "${planner_candidate_query_cells}" \
    "${planner_compiled_cells_emitted}" \
    "${planner_candidate_cells_built}" \
    "${projection_reuse_hits}" \
    "${projection_reuse_misses}" \
    "${planner_cache_hits}" \
    "${planner_cache_misses}" \
    "${planner_validation_compiled_cells_emitted}" \
    "${planner_validation_candidate_cells_built}" \
    "${pool_total_windows}" \
    "${pool_cached_budget}" \
    "${max_kept_windows}" \
    "${pool_peak_requested_capacity}" \
    "${pool_capacity_cap_hits}" \
    "${win_enter_dropped}" \
    "${win_enter_continued}" \
    "${win_scrolled_dropped}" \
    "${win_scrolled_continued}" \
    "${buf_enter_dropped}" \
    "${buf_enter_continued}" \
    >>"${results_tsv}"

  printf '%s %-24s run=%s baseline=%8sus recovery_ratio=%s stress_max_ratio=%s stress_tail_ratio=%s class=%s extmark=%s conceal=%s planner_maps=%s planner_cells=%s envelope_area=%s local_query=%s compiled_query=%s candidate_query=%s compiled=%s candidates=%s reuse_hit=%s reuse_miss=%s cache_hit=%s cache_miss=%s val_compiled=%s val_candidates=%s pool_total=%s pool_cached=%s peak_requested=%s cap_hits=%s max=%s win_enter=%s/%s win_scrolled=%s/%s buf_enter=%s/%s\n' \
    "${side_label}" \
    "${scenario_name}" \
    "${repeat_index}" \
    "${baseline_us}" \
    "${recovery_ratio}" \
    "${stress_max_ratio}" \
    "${stress_tail_ratio}" \
    "${perf_class}" \
    "${extmark_fallback_calls}" \
    "${conceal_full_scan_calls}" \
    "${planner_bucket_maps_scanned}" \
    "${planner_bucket_cells_scanned}" \
    "${planner_local_query_envelope_area_cells}" \
    "${planner_local_query_cells}" \
    "${planner_compiled_query_cells}" \
    "${planner_candidate_query_cells}" \
    "${planner_compiled_cells_emitted}" \
    "${planner_candidate_cells_built}" \
    "${projection_reuse_hits}" \
    "${projection_reuse_misses}" \
    "${planner_cache_hits}" \
    "${planner_cache_misses}" \
    "${planner_validation_compiled_cells_emitted}" \
    "${planner_validation_candidate_cells_built}" \
    "${pool_total_windows}" \
    "${pool_cached_budget}" \
    "${pool_peak_requested_capacity}" \
    "${pool_capacity_cap_hits}" \
    "${max_kept_windows}" \
    "${win_enter_dropped}" \
    "${win_enter_continued}" \
    "${win_scrolled_dropped}" \
    "${win_scrolled_continued}" \
    "${buf_enter_dropped}" \
    "${buf_enter_continued}"
}

run_side() {
  local side_label="$1"
  local side_root="$2"
  local plugin_dir
  local scenario_name
  local repeat_index

  plugin_dir="$(smear_compare_plugin_dir "${side_root}")"
  smear_build_release "${plugin_dir}"
  for scenario_name in "${scenarios[@]}"; do
    for repeat_index in $(seq 1 "${repeats}"); do
      run_once "${side_label}" "${side_root}" "${scenario_name}" "${repeat_index}"
    done
  done
}

render_raw_results() {
  column -t -s $'\t' "${results_tsv}"
}

render_summary_table() {
  {
    printf 'side\tscenario\tavg_baseline_us\tavg_recovery_ratio\tavg_stress_max_ratio\tavg_stress_tail_ratio\tavg_extmark_fallback_calls\tavg_conceal_full_scan_calls\tperf_class\tline_count\n'
    for side_label in local base; do
      for scenario_name in "${scenarios[@]}"; do
        awk -F '\t' -v side="${side_label}" -v scenario="${scenario_name}" '
          NR == 1 { next }
          $1 == side && $2 == scenario {
            baseline_sum += $4
            recovery_sum += $5
            stress_max_sum += $7
            stress_tail_sum += $8
            extmark_sum += $11
            conceal_sum += $12
            count += 1
            perf_class = $9
            line_count = $10
          }
          END {
            if (count > 0) {
              printf "%s\t%s\t%.3f\t%.3f\t%.3f\t%.3f\t%.2f\t%.2f\t%s\t%s\n",
                side,
                scenario,
                baseline_sum / count,
                recovery_sum / count,
                stress_max_sum / count,
                stress_tail_sum / count,
                extmark_sum / count,
                conceal_sum / count,
                perf_class,
                line_count
            }
          }
        ' "${results_tsv}"
      done
    done
  } | column -t -s $'\t'
}

render_worst_case_table() {
  {
    printf 'side\tscenario\tworst_baseline_us\tworst_recovery_ratio\tworst_stress_max_avg_us\tworst_stress_max_ratio\tworst_stress_tail_ratio\tperf_class\tline_count\n'
    for side_label in local base; do
      for scenario_name in "${scenarios[@]}"; do
        awk -F '\t' -v side="${side_label}" -v scenario="${scenario_name}" '
          NR == 1 { next }
          $1 == side && $2 == scenario {
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
            perf_class = $9
            line_count = $10
          }
          END {
            if (count > 0) {
              printf "%s\t%s\t%.3f\t%.3f\t%.3f\t%.3f\t%.3f\t%s\t%s\n",
                side,
                scenario,
                worst_baseline_us,
                worst_recovery_ratio,
                worst_stress_max_avg_us,
                worst_stress_max_ratio,
                worst_stress_tail_ratio,
                perf_class,
                line_count
            }
          }
        ' "${results_tsv}"
      done
    done
  } | column -t -s $'\t'
}

render_planner_telemetry_table() {
  {
    printf 'side\tscenario\tavg_planner_bucket_maps_scanned\tmax_planner_bucket_maps_scanned\tavg_planner_bucket_cells_scanned\tmax_planner_bucket_cells_scanned\tavg_planner_local_query_envelope_area_cells\tmax_planner_local_query_envelope_area_cells\tavg_planner_local_query_cells\tmax_planner_local_query_cells\tavg_planner_compiled_query_cells\tmax_planner_compiled_query_cells\tavg_planner_candidate_query_cells\tmax_planner_candidate_query_cells\tavg_planner_compiled_cells_emitted\tmax_planner_compiled_cells_emitted\tavg_planner_candidate_cells_built\tmax_planner_candidate_cells_built\n'
    for side_label in local base; do
      for scenario_name in "${scenarios[@]}"; do
        awk -F '\t' -v side="${side_label}" -v scenario="${scenario_name}" '
          NR == 1 { next }
          $1 == side && $2 == scenario {
            bucket_maps_sum += $13
            bucket_cells_sum += $14
            envelope_area_sum += $15
            local_query_sum += $16
            compiled_query_sum += $17
            candidate_query_sum += $18
            compiled_sum += $19
            candidate_sum += $20
            if ($13 > bucket_maps_max) {
              bucket_maps_max = $13
            }
            if ($14 > bucket_cells_max) {
              bucket_cells_max = $14
            }
            if ($15 > envelope_area_max) {
              envelope_area_max = $15
            }
            if ($16 > local_query_max) {
              local_query_max = $16
            }
            if ($17 > compiled_query_max) {
              compiled_query_max = $17
            }
            if ($18 > candidate_query_max) {
              candidate_query_max = $18
            }
            if ($19 > compiled_max) {
              compiled_max = $19
            }
            if ($20 > candidate_max) {
              candidate_max = $20
            }
            count += 1
          }
          END {
            if (count > 0) {
              printf "%s\t%s\t%.2f\t%.0f\t%.2f\t%.0f\t%.2f\t%.0f\t%.2f\t%.0f\t%.2f\t%.0f\t%.2f\t%.0f\t%.2f\t%.0f\t%.2f\t%.0f\n",
                side,
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
                candidate_max
            }
          }
        ' "${results_tsv}"
      done
    done
  } | column -t -s $'\t'
}

render_planner_validation_table() {
  {
    printf 'side\tscenario\tavg_projection_reuse_hits\tmax_projection_reuse_hits\tavg_projection_reuse_misses\tmax_projection_reuse_misses\tavg_planner_cache_hits\tmax_planner_cache_hits\tavg_planner_cache_misses\tmax_planner_cache_misses\tavg_validation_planner_compiled_cells_emitted\tmax_validation_planner_compiled_cells_emitted\tavg_validation_planner_candidate_cells_built\tmax_validation_planner_candidate_cells_built\n'
    for side_label in local base; do
      for scenario_name in "${scenarios[@]}"; do
        awk -F '\t' -v side="${side_label}" -v scenario="${scenario_name}" '
          NR == 1 { next }
          $1 == side && $2 == scenario {
            count += 1
            if ($21 ~ /^[0-9.]+$/) {
              reuse_hits_sum += $21
              reuse_hits_count += 1
              if ($21 > reuse_hits_max) {
                reuse_hits_max = $21
              }
            }
            if ($22 ~ /^[0-9.]+$/) {
              reuse_misses_sum += $22
              reuse_misses_count += 1
              if ($22 > reuse_misses_max) {
                reuse_misses_max = $22
              }
            }
            if ($23 ~ /^[0-9.]+$/) {
              cache_hits_sum += $23
              cache_hits_count += 1
              if ($23 > cache_hits_max) {
                cache_hits_max = $23
              }
            }
            if ($24 ~ /^[0-9.]+$/) {
              cache_misses_sum += $24
              cache_misses_count += 1
              if ($24 > cache_misses_max) {
                cache_misses_max = $24
              }
            }
            if ($25 ~ /^[0-9.]+$/) {
              compiled_sum += $25
              compiled_count += 1
              if ($25 > compiled_max) {
                compiled_max = $25
              }
            }
            if ($26 ~ /^[0-9.]+$/) {
              candidate_sum += $26
              candidate_count += 1
              if ($26 > candidate_max) {
                candidate_max = $26
              }
            }
          }
          END {
            if (count > 0) {
              avg_reuse_hits = reuse_hits_count > 0 ? sprintf("%.2f", reuse_hits_sum / reuse_hits_count) : "na"
              max_reuse_hits = reuse_hits_count > 0 ? sprintf("%.0f", reuse_hits_max) : "na"
              avg_reuse_misses = reuse_misses_count > 0 ? sprintf("%.2f", reuse_misses_sum / reuse_misses_count) : "na"
              max_reuse_misses = reuse_misses_count > 0 ? sprintf("%.0f", reuse_misses_max) : "na"
              avg_cache_hits = cache_hits_count > 0 ? sprintf("%.2f", cache_hits_sum / cache_hits_count) : "na"
              max_cache_hits = cache_hits_count > 0 ? sprintf("%.0f", cache_hits_max) : "na"
              avg_cache_misses = cache_misses_count > 0 ? sprintf("%.2f", cache_misses_sum / cache_misses_count) : "na"
              max_cache_misses = cache_misses_count > 0 ? sprintf("%.0f", cache_misses_max) : "na"
              avg_compiled = compiled_count > 0 ? sprintf("%.2f", compiled_sum / compiled_count) : "na"
              max_compiled = compiled_count > 0 ? sprintf("%.0f", compiled_max) : "na"
              avg_candidates = candidate_count > 0 ? sprintf("%.2f", candidate_sum / candidate_count) : "na"
              max_candidates = candidate_count > 0 ? sprintf("%.0f", candidate_max) : "na"

              printf "%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n",
                side,
                scenario,
                avg_reuse_hits,
                max_reuse_hits,
                avg_reuse_misses,
                max_reuse_misses,
                avg_cache_hits,
                max_cache_hits,
                avg_cache_misses,
                max_cache_misses,
                avg_compiled,
                max_compiled,
                avg_candidates,
                max_candidates
            }
          }
        ' "${results_tsv}"
      done
    done
  } | column -t -s $'\t'
}

render_ingress_fast_path_table() {
  {
    printf 'side\tscenario\tavg_win_enter_dropped\tavg_win_enter_continued\tavg_win_enter_drop_pct\tavg_win_scrolled_dropped\tavg_win_scrolled_continued\tavg_win_scrolled_drop_pct\tavg_buf_enter_dropped\tavg_buf_enter_continued\tavg_buf_enter_drop_pct\n'
    for side_label in local base; do
      for scenario_name in "${scenarios[@]}"; do
        awk -F '\t' -v side="${side_label}" -v scenario="${scenario_name}" '
          function ratio_text(dropped_sum, continue_sum, count,    total) {
            total = dropped_sum + continue_sum
            if (count == 0 || total <= 0) {
              return "na"
            }

            return sprintf("%.2f%%", dropped_sum / total * 100.0)
          }

          NR == 1 { next }
          $1 == side && $2 == scenario {
            count += 1
            if ($32 ~ /^[0-9.]+$/) {
              win_enter_dropped_sum += $32
              win_enter_dropped_count += 1
            }
            if ($33 ~ /^[0-9.]+$/) {
              win_enter_continued_sum += $33
              win_enter_continued_count += 1
            }
            if ($34 ~ /^[0-9.]+$/) {
              win_scrolled_dropped_sum += $34
              win_scrolled_dropped_count += 1
            }
            if ($35 ~ /^[0-9.]+$/) {
              win_scrolled_continued_sum += $35
              win_scrolled_continued_count += 1
            }
            if ($36 ~ /^[0-9.]+$/) {
              buf_enter_dropped_sum += $36
              buf_enter_dropped_count += 1
            }
            if ($37 ~ /^[0-9.]+$/) {
              buf_enter_continued_sum += $37
              buf_enter_continued_count += 1
            }
          }
          END {
            if (count > 0) {
              avg_win_enter_dropped = win_enter_dropped_count > 0 ? sprintf("%.2f", win_enter_dropped_sum / win_enter_dropped_count) : "na"
              avg_win_enter_continued = win_enter_continued_count > 0 ? sprintf("%.2f", win_enter_continued_sum / win_enter_continued_count) : "na"
              avg_win_scrolled_dropped = win_scrolled_dropped_count > 0 ? sprintf("%.2f", win_scrolled_dropped_sum / win_scrolled_dropped_count) : "na"
              avg_win_scrolled_continued = win_scrolled_continued_count > 0 ? sprintf("%.2f", win_scrolled_continued_sum / win_scrolled_continued_count) : "na"
              avg_buf_enter_dropped = buf_enter_dropped_count > 0 ? sprintf("%.2f", buf_enter_dropped_sum / buf_enter_dropped_count) : "na"
              avg_buf_enter_continued = buf_enter_continued_count > 0 ? sprintf("%.2f", buf_enter_continued_sum / buf_enter_continued_count) : "na"

              printf "%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n",
                side,
                scenario,
                avg_win_enter_dropped,
                avg_win_enter_continued,
                ratio_text(win_enter_dropped_sum, win_enter_continued_sum, win_enter_dropped_count + win_enter_continued_count),
                avg_win_scrolled_dropped,
                avg_win_scrolled_continued,
                ratio_text(win_scrolled_dropped_sum, win_scrolled_continued_sum, win_scrolled_dropped_count + win_scrolled_continued_count),
                avg_buf_enter_dropped,
                avg_buf_enter_continued,
                ratio_text(buf_enter_dropped_sum, buf_enter_continued_sum, buf_enter_dropped_count + buf_enter_continued_count)
            }
          }
        ' "${results_tsv}"
      done
    done
  } | column -t -s $'\t'
}

render_pool_retention_table() {
  {
    printf 'side\tscenario\tavg_pool_total_windows\tavg_pool_cached_budget\tmax_kept_windows\tavg_pool_total_pct_of_max\tavg_pool_cached_budget_pct_of_max\n'
    for side_label in local base; do
      for scenario_name in "${scenarios[@]}"; do
        awk -F '\t' -v side="${side_label}" -v scenario="${scenario_name}" '
          NR == 1 { next }
          $1 == side && $2 == scenario {
            count += 1
            if ($27 ~ /^[0-9.]+$/) {
              pool_total_sum += $27
              pool_total_count += 1
            }
            if ($28 ~ /^[0-9.]+$/) {
              pool_cached_sum += $28
              pool_cached_count += 1
            }
            if ($29 ~ /^[0-9.]+$/ && ($29 + 0) > 0) {
              max_kept_windows = $29 + 0
            }
          }
          END {
            if (count > 0) {
              if (pool_total_count > 0) {
                avg_pool_total = sprintf("%.2f", pool_total_sum / pool_total_count)
              } else {
                avg_pool_total = "na"
              }

              if (pool_cached_count > 0) {
                avg_pool_cached = sprintf("%.2f", pool_cached_sum / pool_cached_count)
              } else {
                avg_pool_cached = "na"
              }

              if (max_kept_windows > 0) {
                max_kept_windows_text = sprintf("%d", max_kept_windows)
              } else {
                max_kept_windows_text = "na"
              }

              if (max_kept_windows > 0 && pool_total_count > 0) {
                total_pct = sprintf("%.2f", (pool_total_sum / pool_total_count) / max_kept_windows * 100.0)
              } else {
                total_pct = "na"
              }

              if (max_kept_windows > 0 && pool_cached_count > 0) {
                cached_pct = sprintf("%.2f", (pool_cached_sum / pool_cached_count) / max_kept_windows * 100.0)
              } else {
                cached_pct = "na"
              }

              printf "%s\t%s\t%s\t%s\t%s\t%s\t%s\n",
                side,
                scenario,
                avg_pool_total,
                avg_pool_cached,
                max_kept_windows_text,
                total_pct,
                cached_pct
            }
          }
        ' "${results_tsv}"
      done
    done
  } | column -t -s $'\t'
}

render_pool_peak_pressure_table() {
  {
    printf 'side\tscenario\tavg_pool_peak_requested_capacity\tmax_pool_capacity_cap_hits\tmax_kept_windows\tavg_pool_peak_requested_pct_of_max\n'
    for side_label in local base; do
      for scenario_name in "${scenarios[@]}"; do
        awk -F '\t' -v side="${side_label}" -v scenario="${scenario_name}" '
          NR == 1 { next }
          $1 == side && $2 == scenario {
            count += 1
            if ($30 ~ /^[0-9.]+$/) {
              peak_requested_sum += $30
              peak_requested_count += 1
            }
            if ($31 ~ /^[0-9.]+$/ && ($31 + 0) > max_cap_hits) {
              max_cap_hits = $31 + 0
            }
            if ($29 ~ /^[0-9.]+$/ && ($29 + 0) > 0) {
              max_kept_windows = $29 + 0
            }
          }
          END {
            if (count > 0) {
              if (peak_requested_count > 0) {
                avg_peak_requested = sprintf("%.2f", peak_requested_sum / peak_requested_count)
              } else {
                avg_peak_requested = "na"
              }

              if (max_kept_windows > 0) {
                max_kept_windows_text = sprintf("%d", max_kept_windows)
              } else {
                max_kept_windows_text = "na"
              }

              if (max_kept_windows > 0 && peak_requested_count > 0) {
                peak_requested_pct = sprintf("%.2f", (peak_requested_sum / peak_requested_count) / max_kept_windows * 100.0)
              } else {
                peak_requested_pct = "na"
              }

              printf "%s\t%s\t%s\t%d\t%s\t%s\n",
                side,
                scenario,
                avg_peak_requested,
                max_cap_hits,
                max_kept_windows_text,
                peak_requested_pct
            }
          }
        ' "${results_tsv}"
      done
    done
  } | column -t -s $'\t'
}

render_particle_isolation_table() {
  if ! has_scenario "particles_off" || ! has_scenario "particles_on"; then
    return
  fi

  awk -F '\t' '
    NR == 1 { next }
    ($2 == "particles_off" || $2 == "particles_on") {
      baseline_sum[$1 "|" $2] += $4
      recovery_sum[$1 "|" $2] += $5
      stress_max_sum[$1 "|" $2] += $7
      stress_tail_sum[$1 "|" $2] += $8
      count[$1 "|" $2] += 1
    }
    END {
      printf "side\tparticles_off_avg_baseline_us\tparticles_on_avg_baseline_us\tparticle_tax_pct\tparticles_off_avg_recovery_ratio\tparticles_on_avg_recovery_ratio\tparticles_off_avg_stress_max_ratio\tparticles_on_avg_stress_max_ratio\tparticles_off_avg_stress_tail_ratio\tparticles_on_avg_stress_tail_ratio\n"
      for (side_index = 1; side_index <= 2; side_index++) {
        side = side_index == 1 ? "local" : "base"
        off_key = side "|particles_off"
        on_key = side "|particles_on"
        if (count[off_key] == 0 || count[on_key] == 0) {
          continue
        }

        off_baseline = baseline_sum[off_key] / count[off_key]
        on_baseline = baseline_sum[on_key] / count[on_key]
        particle_tax_pct = (on_baseline - off_baseline) / off_baseline * 100.0
        off_recovery = recovery_sum[off_key] / count[off_key]
        on_recovery = recovery_sum[on_key] / count[on_key]
        off_stress_max = stress_max_sum[off_key] / count[off_key]
        on_stress_max = stress_max_sum[on_key] / count[on_key]
        off_stress_tail = stress_tail_sum[off_key] / count[off_key]
        on_stress_tail = stress_tail_sum[on_key] / count[on_key]

        printf "%s\t%.3f\t%.3f\t%+.2f%%\t%.3f\t%.3f\t%.3f\t%.3f\t%.3f\t%.3f\n",
          side,
          off_baseline,
          on_baseline,
          particle_tax_pct,
          off_recovery,
          on_recovery,
          off_stress_max,
          on_stress_max,
          off_stress_tail,
          on_stress_tail
      }
    }
  ' "${results_tsv}" | column -t -s $'\t'
}

render_delta_table() {
  {
    printf 'scenario\tlocal_avg_baseline_us\tbase_avg_baseline_us\tbaseline_delta_pct\tlocal_avg_recovery_ratio\tbase_avg_recovery_ratio\tlocal_avg_stress_max_ratio\tbase_avg_stress_max_ratio\tlocal_avg_stress_tail_ratio\tbase_avg_stress_tail_ratio\n'
    for scenario_name in "${scenarios[@]}"; do
      awk -F '\t' -v scenario="${scenario_name}" '
        NR == 1 { next }
        $2 == scenario {
          baseline_sum[$1] += $4
          recovery_sum[$1] += $5
          stress_max_sum[$1] += $7
          stress_tail_sum[$1] += $8
          count[$1] += 1
        }
        END {
          local_baseline = baseline_sum["local"] / count["local"]
          base_baseline = baseline_sum["base"] / count["base"]
          local_recovery = recovery_sum["local"] / count["local"]
          base_recovery = recovery_sum["base"] / count["base"]
          local_stress_max = stress_max_sum["local"] / count["local"]
          base_stress_max = stress_max_sum["base"] / count["base"]
          local_stress_tail = stress_tail_sum["local"] / count["local"]
          base_stress_tail = stress_tail_sum["base"] / count["base"]
          baseline_delta_pct = (local_baseline - base_baseline) / base_baseline * 100.0
          printf "%s\t%.3f\t%.3f\t%+.2f%%\t%.3f\t%.3f\t%.3f\t%.3f\t%.3f\t%.3f\n",
            scenario,
            local_baseline,
            base_baseline,
            baseline_delta_pct,
            local_recovery,
            base_recovery,
            local_stress_max,
            base_stress_max,
            local_stress_tail,
            base_stress_tail
        }
      ' "${results_tsv}"
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

  git_commit="$(smear_report_git_commit "${repo_root}")"
  git_state="$(smear_report_git_state "${repo_root}")"
  nvim_version="$(smear_report_nvim_version)"
  capture_time="$(smear_report_capture_time_utc)"
  command_line="${report_command:-SMEAR_COMPARE_REPORT_FILE=${output_file} ${rust_repo_dir}/plugins/smear-cursor/scripts/compare_window_switch_scenarios.sh ${base_ref}}"

  mkdir -p "$(dirname -- "${output_file}")"
  {
    printf '# Window Switch Scenario Perf Snapshot\n\n'
    printf -- '- Captured (UTC): %s\n' "${capture_time}"
    printf -- '- Repo commit: `%s`\n' "${git_commit}"
    printf -- '- Working tree: `%s`\n' "${git_state}"
    printf -- '- Neovim: `%s`\n' "${nvim_version}"
    printf -- '- Base ref: `%s`\n' "${base_ref}"
    printf -- '- Command: `%s`\n' "${command_line}"
    printf -- '- Local overrides: `%s`\n' "${local_overrides_raw:-none}"
    printf -- '- Base overrides: `%s`\n' "${base_overrides_raw:-none}"
    printf -- '- Config: repeats=`%s`, scenarios=`%s`, warmup=`%s`, baseline=`%s`, stress=`%s`, stress_rounds=`%s`, recovery=`%s`, recovery_mode=`%s`, settle_wait_ms=`%s`, windows=`%s`, drain_every=`%s`, delay_event_to_smear=`%s`\n' \
      "${repeats}" \
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
    if awk 'NR == 1 { next } { for (i = 3; i <= NF; i += 1) if ($i != "na") found = 1 } END { exit found ? 0 : 1 }' "${planner_validation_table}"; then
      printf '\n## Planner Validation Counters\n\n```text\n'
      cat "${planner_validation_table}"
      printf '```\n'
    fi
    if awk 'NR == 1 { next } { for (i = 3; i <= NF; i += 1) if ($i != "na") found = 1 } END { exit found ? 0 : 1 }' "${ingress_fast_path_table}"; then
      printf '\n## Ingress Fast-Path Counters\n\n```text\n'
      cat "${ingress_fast_path_table}"
      printf '```\n'
    fi
    printf '\n## Pool Retention vs max_kept_windows\n\n```text\n'
    cat "${pool_retention_table}"
    printf '```\n\n## Pool Peak Pressure vs max_kept_windows\n\n```text\n'
    cat "${pool_peak_pressure_table}"
    printf '```\n'
    if [[ -s "${particle_isolation_table}" ]]; then
      printf '\n## Particle Isolation (same side)\n\n```text\n'
      cat "${particle_isolation_table}"
      printf '```\n'
    fi
    printf '\n## Delta (local vs base)\n\n```text\n'
    cat "${delta_table}"
    printf '```\n'
  } >"${output_file}"
}

printf 'side\tscenario\trun\tbaseline_us\trecovery_ratio\tstress_max_avg_us\tstress_max_ratio\tstress_tail_ratio\tperf_class\tline_count\textmark_fallback_calls\tconceal_full_scan_calls\tplanner_bucket_maps_scanned\tplanner_bucket_cells_scanned\tplanner_local_query_envelope_area_cells\tplanner_local_query_cells\tplanner_compiled_query_cells\tplanner_candidate_query_cells\tplanner_compiled_cells_emitted\tplanner_candidate_cells_built\tprojection_reuse_hits\tprojection_reuse_misses\tplanner_cache_hits\tplanner_cache_misses\tvalidation_planner_compiled_cells_emitted\tvalidation_planner_candidate_cells_built\tpool_total_windows\tpool_cached_budget\tmax_kept_windows\tpool_peak_requested_capacity\tpool_capacity_cap_hits\twin_enter_dropped\twin_enter_continued\twin_scrolled_dropped\twin_scrolled_continued\tbuf_enter_dropped\tbuf_enter_continued\n' >"${results_tsv}"

echo "base_ref=${base_ref}"
echo "local_root=${repo_root}"
echo "base_root=${worktree_dir}"
echo "artifacts=${artifact_dir}"
echo "scenarios=${scenario_csv}"
echo "local_overrides=${local_overrides_raw:-none}"
echo "base_overrides=${base_overrides_raw:-none}"
echo

smear_build_perf_report_tool "${rust_repo_dir}"
run_side "local" "${repo_root}"
run_side "base" "${worktree_dir}"

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

echo
echo "== Planner Validation Counters =="
render_planner_validation_table | tee "${planner_validation_table}"

echo
echo "== Ingress Fast-Path Counters =="
render_ingress_fast_path_table | tee "${ingress_fast_path_table}"

echo
echo "== Pool Retention vs max_kept_windows =="
render_pool_retention_table | tee "${pool_retention_table}"

echo
echo "== Pool Peak Pressure vs max_kept_windows =="
render_pool_peak_pressure_table | tee "${pool_peak_pressure_table}"

if has_scenario "particles_off" && has_scenario "particles_on"; then
  echo
  echo "== Particle Isolation (same side) =="
  render_particle_isolation_table | tee "${particle_isolation_table}"
else
  : >"${particle_isolation_table}"
fi

echo
echo "== Delta (local vs base) =="
render_delta_table | tee "${delta_table}"

if [[ -n "${report_file}" ]]; then
  write_report "${report_file}"
  echo
  echo "report=${report_file}"
fi

#!/usr/bin/env bash
set -euo pipefail

# Capture long-animation CPU and allocation baselines with particle effects enabled.
#
# This keeps the current tree fixed and measures the active baseline animation window by taking
# deltas between `PERF_VALIDATION phase=post_warmup` and `phase=post_baseline`.
#
# Usage:
#   scripts/capture_long_animation_allocations.sh
#
# Tunables:
#   SMEAR_LONG_ANIMATION_REPORT_FILE       (optional; write a markdown snapshot report)
#   SMEAR_LONG_ANIMATION_REPEATS           (default: 2)
#   SMEAR_LONG_ANIMATION_SCENARIO          (default: long_running_repetition)
#   SMEAR_BUFFER_PERF_MODE                 (default: full)
#   SMEAR_LONG_ANIMATION_PARTICLES_ENABLED (default: true)
#   SMEAR_LONG_ANIMATION_WARMUP            (default: 300)
#   SMEAR_LONG_ANIMATION_BASELINE          (default: 1200)
#   SMEAR_LONG_ANIMATION_WINDOWS           (default: 8)
#   SMEAR_LONG_ANIMATION_DRAIN_EVERY       (default: 1)

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

repeats="${SMEAR_LONG_ANIMATION_REPEATS:-2}"
scenario_name="${SMEAR_LONG_ANIMATION_SCENARIO:-long_running_repetition}"
buffer_perf_mode="${SMEAR_BUFFER_PERF_MODE:-full}"
particles_enabled="${SMEAR_LONG_ANIMATION_PARTICLES_ENABLED:-true}"
warmup_iterations="${SMEAR_LONG_ANIMATION_WARMUP:-300}"
baseline_iterations="${SMEAR_LONG_ANIMATION_BASELINE:-1200}"
windows_count="${SMEAR_LONG_ANIMATION_WINDOWS:-8}"
drain_every="${SMEAR_LONG_ANIMATION_DRAIN_EVERY:-1}"
report_file="${SMEAR_LONG_ANIMATION_REPORT_FILE:-}"
export SMEAR_CARGO_FEATURES="${SMEAR_CARGO_FEATURES:-perf-counters}"

artifact_dir="$(mktemp -d /tmp/smear_long_animation_allocations.XXXXXX)"
results_tsv="${artifact_dir}/long_animation_allocations.tsv"
raw_results_table="${artifact_dir}/long_animation_allocations_raw.txt"
summary_table="${artifact_dir}/long_animation_allocations_summary.txt"

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
  local repeat_index="$1"
  local log_file="${artifact_dir}/run_${scenario_name}_${repeat_index}.log"
  local baseline_line
  local diagnostics_line
  local warmup_validation_line
  local baseline_validation_line
  local baseline_elapsed_ms
  local baseline_avg_us
  local perf_class
  local probe_policy
  local delta_particle_simulation_steps
  local delta_particle_aggregation_calls
  local delta_planning_preview_invocations
  local delta_planning_preview_copied_particles
  local delta_particle_overlay_refreshes
  local delta_allocation_ops
  local allocation_ops_per_s
  local delta_allocation_bytes
  local allocation_bytes_per_s

  (
    cd "${repo_dir}"
    env \
      "SMEAR_CURSOR_RTP=${SMEAR_CURSOR_RTP}" \
      "SMEAR_CURSOR_CPATH=${SMEAR_CURSOR_CPATH}" \
      "SMEAR_SCENARIO_NAME=${scenario_name}" \
      "SMEAR_SCENARIO_PRESET=${scenario_name}" \
      "SMEAR_ENABLE_ALLOCATION_COUNTERS=1" \
      "SMEAR_PARTICLES_ENABLED=${particles_enabled}" \
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

  baseline_line="$(grep 'PERF_PHASE name=baseline ' "${log_file}" | tail -n 1)"
  diagnostics_line="$(grep 'PERF_DIAGNOSTICS phase=post_baseline ' "${log_file}" | tail -n 1)"
  warmup_validation_line="$(grep 'PERF_VALIDATION phase=post_warmup ' "${log_file}" | tail -n 1)"
  baseline_validation_line="$(grep 'PERF_VALIDATION phase=post_baseline ' "${log_file}" | tail -n 1)"
  if [[ -z "${baseline_line}" || -z "${diagnostics_line}" || -z "${warmup_validation_line}" || -z "${baseline_validation_line}" ]]; then
    echo "missing long-animation allocation fields in ${log_file}" >&2
    exit 1
  fi

  baseline_elapsed_ms="$(smear_extract_field "${baseline_line}" "elapsed_ms")"
  baseline_avg_us="$(smear_extract_field "${baseline_line}" "avg_us")"
  perf_class="$(smear_extract_field "${diagnostics_line}" "perf_class")"
  probe_policy="$(smear_extract_field "${diagnostics_line}" "probe_policy")"

  delta_particle_simulation_steps="$(
    calc_delta \
      "$(smear_extract_field "${baseline_validation_line}" "pss")" \
      "$(smear_extract_field "${warmup_validation_line}" "pss")"
  )"
  delta_particle_aggregation_calls="$(
    calc_delta \
      "$(smear_extract_field "${baseline_validation_line}" "pac")" \
      "$(smear_extract_field "${warmup_validation_line}" "pac")"
  )"
  delta_planning_preview_invocations="$(
    calc_delta \
      "$(smear_extract_field "${baseline_validation_line}" "ppi")" \
      "$(smear_extract_field "${warmup_validation_line}" "ppi")"
  )"
  delta_planning_preview_copied_particles="$(
    calc_delta \
      "$(smear_extract_field "${baseline_validation_line}" "ppp")" \
      "$(smear_extract_field "${warmup_validation_line}" "ppp")"
  )"
  delta_particle_overlay_refreshes="$(
    calc_delta \
      "$(smear_extract_field "${baseline_validation_line}" "por")" \
      "$(smear_extract_field "${warmup_validation_line}" "por")"
  )"
  delta_allocation_ops="$(
    calc_delta \
      "$(smear_extract_field "${baseline_validation_line}" "alc")" \
      "$(smear_extract_field "${warmup_validation_line}" "alc")"
  )"
  allocation_ops_per_s="$(calc_rate_per_s "${delta_allocation_ops}" "${baseline_elapsed_ms}")"
  delta_allocation_bytes="$(
    calc_delta \
      "$(smear_extract_field "${baseline_validation_line}" "alb")" \
      "$(smear_extract_field "${warmup_validation_line}" "alb")"
  )"
  allocation_bytes_per_s="$(calc_rate_per_s "${delta_allocation_bytes}" "${baseline_elapsed_ms}")"

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "${scenario_name}" \
    "${repeat_index}" \
    "${baseline_elapsed_ms}" \
    "${baseline_avg_us}" \
    "${perf_class}" \
    "${probe_policy}" \
    "${delta_particle_simulation_steps}" \
    "${delta_particle_aggregation_calls}" \
    "${delta_planning_preview_invocations}" \
    "${delta_planning_preview_copied_particles}" \
    "${delta_particle_overlay_refreshes}" \
    "${delta_allocation_ops}" \
    "${allocation_ops_per_s}" \
    "${delta_allocation_bytes}" \
    "${allocation_bytes_per_s}" \
    >>"${results_tsv}"

  printf '%-24s run=%s baseline_ms=%s alloc_ops/s=%s alloc_bytes/s=%s particle_steps=%s aggregation_calls=%s preview_calls=%s preview_copied_particles=%s overlay_refreshes=%s\n' \
    "${scenario_name}" \
    "${repeat_index}" \
    "${baseline_elapsed_ms}" \
    "${allocation_ops_per_s}" \
    "${allocation_bytes_per_s}" \
    "${delta_particle_simulation_steps}" \
    "${delta_particle_aggregation_calls}" \
    "${delta_planning_preview_invocations}" \
    "${delta_planning_preview_copied_particles}" \
    "${delta_particle_overlay_refreshes}"
}

render_raw_results() {
  column -t -s $'\t' "${results_tsv}"
}

render_summary_table() {
  awk -F '\t' '
    NR == 1 { next }
    {
      baseline_ms_sum += $3
      baseline_us_sum += $4
      particle_steps_sum += $7
      aggregation_calls_sum += $8
      planning_preview_invocations_sum += $9
      planning_preview_copied_particles_sum += $10
      overlay_refreshes_sum += $11
      allocation_ops_sum += $12
      allocation_ops_per_s_sum += $13
      allocation_bytes_sum += $14
      allocation_bytes_per_s_sum += $15
      count += 1
      scenario = $1
      perf_class = $5
      probe_policy = $6
    }
    END {
      if (count > 0) {
        printf "scenario\tavg_baseline_ms\tavg_baseline_us\tperf_class\tprobe_policy\tavg_particle_simulation_steps\tavg_particle_aggregation_calls\tavg_planning_preview_invocations\tavg_planning_preview_copied_particles\tavg_particle_overlay_refreshes\tavg_allocation_ops\tavg_allocation_ops_per_s\tavg_allocation_bytes\tavg_allocation_bytes_per_s\n"
        printf "%s\t%.3f\t%.3f\t%s\t%s\t%.1f\t%.1f\t%.1f\t%.1f\t%.1f\t%.1f\t%.3f\t%.1f\t%.3f\n",
          scenario,
          baseline_ms_sum / count,
          baseline_us_sum / count,
          perf_class,
          probe_policy,
          particle_steps_sum / count,
          aggregation_calls_sum / count,
          planning_preview_invocations_sum / count,
          planning_preview_copied_particles_sum / count,
          overlay_refreshes_sum / count,
          allocation_ops_sum / count,
          allocation_ops_per_s_sum / count,
          allocation_bytes_sum / count,
          allocation_bytes_per_s_sum / count
      }
    }
  ' "${results_tsv}" | column -t -s $'\t'
}

printf 'scenario\trun\tbaseline_elapsed_ms\tbaseline_avg_us\tperf_class\tprobe_policy\tparticle_simulation_steps\tparticle_aggregation_calls\tplanning_preview_invocations\tplanning_preview_copied_particles\tparticle_overlay_refreshes\tallocation_ops\tallocation_ops_per_s\tallocation_bytes\tallocation_bytes_per_s\n' >"${results_tsv}"

echo "repo=${repo_dir}"
echo "artifacts=${artifact_dir}"
echo "scenario=${scenario_name}"
echo "buffer_perf_mode=${buffer_perf_mode}"
echo "particles_enabled=${particles_enabled}"
echo

smear_build_release "${repo_dir}"
if ! smear_export_runtime_paths "${repo_dir}" >/dev/null; then
  echo "failed to resolve runtime paths for ${repo_dir}" >&2
  exit 1
fi

for repeat_index in $(seq 1 "${repeats}"); do
  run_once "${repeat_index}"
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
# Long Animation Allocation Snapshot

- Captured (UTC): \`$(smear_report_capture_time_utc)\`
- Repo commit: \`$(smear_report_git_commit "${repo_dir}")\`
- Working tree: \`$(smear_report_git_state "${repo_dir}")\`
- Neovim: \`$(smear_report_nvim_version)\`
- Command: \`SMEAR_LONG_ANIMATION_REPORT_FILE=${report_file} ${repo_dir}/scripts/capture_long_animation_allocations.sh\`
- Config: repeats=\`${repeats}\`, scenario=\`${scenario_name}\`, buffer_perf_mode=\`${buffer_perf_mode}\`, particles_enabled=\`${particles_enabled}\`, warmup=\`${warmup_iterations}\`, baseline=\`${baseline_iterations}\`, windows=\`${windows_count}\`, drain_every=\`${drain_every}\`

These rates use the delta between \`PERF_VALIDATION phase=post_warmup\` and
\`PERF_VALIDATION phase=post_baseline\` so the allocation counts represent the
active long-animation window rather than cumulative plugin lifetime totals.

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

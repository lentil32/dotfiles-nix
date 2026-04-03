#!/usr/bin/env bash
set -euo pipefail

# Compare particle-path performance for local working tree vs a base git ref.
# This script uses the current perf harness driver (perf_window_switch.lua) so
# particles can be toggled through env vars for both sides.
#
# Usage:
#   scripts/compare_particle_probe_perf.sh            # base_ref defaults to HEAD
#   scripts/compare_particle_probe_perf.sh HEAD~1
#
# Tunables:
#   SMEAR_COMPARE_REPEATS            (default: 3)
#   SMEAR_COMPARE_WARMUP             (default: 150)
#   SMEAR_COMPARE_BASELINE           (default: 800)
#   SMEAR_COMPARE_STRESS             (default: 1600)
#   SMEAR_COMPARE_STRESS_ROUNDS      (default: 1)
#   SMEAR_COMPARE_RECOVERY           (default: 800)
#   SMEAR_COMPARE_SETTLE_WAIT_MS     (default: 300)
#   SMEAR_COMPARE_WINDOWS            (default: 8)
#   SMEAR_COMPARE_DRAIN_EVERY        (default: 1)

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
repeats="${SMEAR_COMPARE_REPEATS:-3}"

warmup_iterations="${SMEAR_COMPARE_WARMUP:-150}"
baseline_iterations="${SMEAR_COMPARE_BASELINE:-800}"
stress_iterations="${SMEAR_COMPARE_STRESS:-1600}"
stress_rounds="${SMEAR_COMPARE_STRESS_ROUNDS:-1}"
recovery_iterations="${SMEAR_COMPARE_RECOVERY:-800}"
settle_wait_ms="${SMEAR_COMPARE_SETTLE_WAIT_MS:-300}"
windows_count="${SMEAR_COMPARE_WINDOWS:-8}"
drain_every="${SMEAR_COMPARE_DRAIN_EVERY:-1}"

worktree_dir="$(mktemp -d /tmp/smear_probe_compare.XXXXXX)"
artifact_dir="$(mktemp -d /tmp/smear_probe_compare_artifacts.XXXXXX)"
results_tsv="${artifact_dir}/probe_compare_results.tsv"

cleanup() {
  git -C "${repo_root}" worktree remove "${worktree_dir}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

git -C "${repo_root}" worktree add --detach "${worktree_dir}" "${base_ref}" >/dev/null

build_release() {
  local plugin_dir="$1"
  (
    cd "${plugin_dir}"
    cargo build --release >/dev/null
  )
}

run_once() {
  local side_label="$1"
  local side_root="$2"
  local case_label="$3"
  local particles_over_text="$4"
  local repeat_index="$5"
  local plugin_dir="${side_root}/nvim/rust/plugins/smear-cursor"
  local log_file="${artifact_dir}/run_${side_label}_${case_label}_${repeat_index}.log"
  local target_directory
  local smear_cursor_cpath
  target_directory="$(smear_resolve_target_directory "${plugin_dir}")"

  if [[ -z "${target_directory}" ]]; then
    echo "failed to resolve target_directory for ${side_label}" >&2
    exit 1
  fi
  smear_cursor_cpath="$(smear_default_cpath "${target_directory}")"

  (
    cd "${plugin_dir}"
    SMEAR_CURSOR_RTP="${plugin_dir}" \
      SMEAR_CURSOR_CPATH="${smear_cursor_cpath}" \
      SMEAR_WINDOWS="${windows_count}" \
      SMEAR_WARMUP_ITERATIONS="${warmup_iterations}" \
      SMEAR_BASELINE_ITERATIONS="${baseline_iterations}" \
      SMEAR_STRESS_ITERATIONS="${stress_iterations}" \
      SMEAR_STRESS_ROUNDS="${stress_rounds}" \
      SMEAR_RECOVERY_ITERATIONS="${recovery_iterations}" \
      SMEAR_SETTLE_WAIT_MS="${settle_wait_ms}" \
      SMEAR_BETWEEN_BUFFERS=true \
      SMEAR_PARTICLES_ENABLED=true \
      SMEAR_PARTICLES_OVER_TEXT="${particles_over_text}" \
      SMEAR_DRAIN_EVERY="${drain_every}" \
      SMEAR_DELAY_EVENT_TO_SMEAR=0 \
      "${NVIM_BIN:-nvim}" --headless -u NONE -c "luafile ${driver_lua}"
  ) >"${log_file}" 2>&1

  local summary_line
  summary_line="$(grep 'PERF_SUMMARY' "${log_file}" | tail -n 1)"
  local baseline_us
  local recovery_us
  local recovery_ratio
  baseline_us="$(printf '%s\n' "${summary_line}" | sed -E 's/.*baseline_avg_us=([0-9.]+).*/\1/')"
  recovery_us="$(printf '%s\n' "${summary_line}" | sed -E 's/.*recovery_avg_us=([0-9.]+).*/\1/')"
  recovery_ratio="$(printf '%s\n' "${summary_line}" | sed -E 's/.*recovery_ratio=([0-9.]+).*/\1/')"

  printf '%s\t%s\t%s\t%s\t%s\n' \
    "${side_label}" "${case_label}" "${repeat_index}" "${baseline_us}" "${recovery_ratio}" \
    >>"${results_tsv}"

  printf '%s %-8s run=%s baseline=%8sus recovery=%8sus ratio=%s\n' \
    "${side_label}" "${case_label}" "${repeat_index}" "${baseline_us}" "${recovery_us}" "${recovery_ratio}"
}

run_side() {
  local side_label="$1"
  local side_root="$2"
  local plugin_dir="${side_root}/nvim/rust/plugins/smear-cursor"

  build_release "${plugin_dir}"
  for repeat_index in $(seq 1 "${repeats}"); do
    run_once "${side_label}" "${side_root}" "probe_on" "false" "${repeat_index}"
  done
  for repeat_index in $(seq 1 "${repeats}"); do
    run_once "${side_label}" "${side_root}" "probe_off" "true" "${repeat_index}"
  done
}

printf 'side\tcase\trun\tbaseline_us\trecovery_ratio\n' >"${results_tsv}"

echo "base_ref=${base_ref}"
echo "local_root=${repo_root}"
echo "base_root=${worktree_dir}"
echo "artifacts=${artifact_dir}"
echo

run_side "local" "${repo_root}"
run_side "base" "${worktree_dir}"

echo
echo "== Raw Results =="
column -t -s $'\t' "${results_tsv}"

echo
echo "== Summary (avg baseline) =="
awk -F '\t' '
  NR == 1 { next }
  {
    key = $1 "|" $2
    baseline_sum[key] += $4
    count[key] += 1
  }
  END {
    printf "side\tcase\tavg_baseline_us\n"
    for (key in baseline_sum) {
      split(key, parts, "|")
      avg = baseline_sum[key] / count[key]
      printf "%s\t%s\t%.3f\n", parts[1], parts[2], avg
    }
  }
' "${results_tsv}" | column -t -s $'\t'

echo
echo "== Delta (local vs base) =="
awk -F '\t' '
  NR == 1 { next }
  {
    key = $1 "|" $2
    baseline_sum[key] += $4
    count[key] += 1
  }
  END {
    local_probe_on = baseline_sum["local|probe_on"] / count["local|probe_on"]
    base_probe_on = baseline_sum["base|probe_on"] / count["base|probe_on"]
    local_probe_off = baseline_sum["local|probe_off"] / count["local|probe_off"]
    base_probe_off = baseline_sum["base|probe_off"] / count["base|probe_off"]

    delta_probe_on = (local_probe_on - base_probe_on) / base_probe_on * 100.0
    delta_probe_off = (local_probe_off - base_probe_off) / base_probe_off * 100.0

    printf "case\tlocal_avg_us\tbase_avg_us\tdelta_pct\n"
    printf "probe_on\t%.3f\t%.3f\t%+.2f%%\n", local_probe_on, base_probe_on, delta_probe_on
    printf "probe_off\t%.3f\t%.3f\t%+.2f%%\n", local_probe_off, base_probe_off, delta_probe_off
  }
' "${results_tsv}" | column -t -s $'\t'

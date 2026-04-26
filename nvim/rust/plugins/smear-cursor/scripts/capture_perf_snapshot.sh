#!/usr/bin/env bash
set -euo pipefail

# Capture the canonical smear-cursor perf snapshot.
#
# The checked-in perf surface is intentionally one markdown file. The lower-level
# comparison scripts remain useful as probes, but this script is the supported
# entrypoint for refreshing repository perf evidence.
#
# Usage:
#   scripts/capture_perf_snapshot.sh
#   scripts/capture_perf_snapshot.sh HEAD~1
#
# Tunables:
#   SMEAR_PERF_REPORT_FILE        (default: perf/current.md)
#   SMEAR_PERF_BASE_REF           (default: HEAD; overridden by first arg)
#   SMEAR_PERF_REPEATS            (default: 2)
#   SMEAR_PERF_BUFFER_MODES       (default: auto,full,fast)
#   SMEAR_PERF_BUFFER_SCENARIOS   (default: large_line_count,long_running_repetition,extmark_heavy,conceal_heavy,particles_on)
#   SMEAR_PERF_PLANNER_MODES      (default: reference,local_query)
#   SMEAR_PERF_PLANNER_SCENARIOS  (default: planner_heavy)
#   SMEAR_PERF_CAP_SCENARIOS      (default: large_line_count,long_running_repetition,planner_heavy,extmark_heavy,conceal_heavy)
#   SMEAR_PERF_PARTICLE_REPEATS   (default: SMEAR_PERF_REPEATS)

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd -- "${script_dir}/.." && pwd)"
workspace_dir="$(cd -- "${repo_dir}/../.." && pwd)"

base_ref="${1:-${SMEAR_PERF_BASE_REF:-HEAD}}"
report_file="${SMEAR_PERF_REPORT_FILE:-${repo_dir}/perf/current.md}"
repeats="${SMEAR_PERF_REPEATS:-2}"
particle_repeats="${SMEAR_PERF_PARTICLE_REPEATS:-${repeats}}"
buffer_modes="${SMEAR_PERF_BUFFER_MODES:-auto,full,fast}"
buffer_scenarios="${SMEAR_PERF_BUFFER_SCENARIOS:-large_line_count,long_running_repetition,extmark_heavy,conceal_heavy,particles_on}"
planner_modes="${SMEAR_PERF_PLANNER_MODES:-reference,local_query}"
planner_scenarios="${SMEAR_PERF_PLANNER_SCENARIOS:-planner_heavy}"
cap_scenarios="${SMEAR_PERF_CAP_SCENARIOS:-large_line_count,long_running_repetition,planner_heavy,extmark_heavy,conceal_heavy}"

artifact_dir="$(mktemp -d /tmp/smear_perf_snapshot.XXXXXX)"
adaptive_report="${artifact_dir}/adaptive-buffer-policy.md"
planner_report="${artifact_dir}/planner-compile.md"
particle_report="${artifact_dir}/particle-toggle.md"
window_cap_report="${artifact_dir}/window-pool-cap.md"

working_tree_state() {
  if git -C "${workspace_dir}" diff --quiet --ignore-submodules -- \
    && git -C "${workspace_dir}" diff --cached --quiet --ignore-submodules --; then
    printf 'clean'
  else
    printf 'dirty'
  fi
}

append_report_body() {
  local title="$1"
  local source_file="$2"

  {
    printf '## %s\n\n' "${title}"
    awk '
      /^## / {
        emit = 1
        sub(/^## /, "### ")
      }
      emit {
        print
      }
    ' "${source_file}"
    printf '\n'
  } >>"${report_file}"
}

canonical_command="SMEAR_PERF_REPORT_FILE=plugins/smear-cursor/perf/current.md ${repo_dir}/scripts/capture_perf_snapshot.sh ${base_ref}"

mkdir -p "$(dirname -- "${report_file}")"

echo "repo=${repo_dir}"
echo "artifacts=${artifact_dir}"
echo "report=${report_file}"
echo "base_ref=${base_ref}"
echo

echo "== adaptive buffer policy =="
SMEAR_COMPARE_REPEATS="${repeats}" \
  SMEAR_COMPARE_MODES="${buffer_modes}" \
  SMEAR_COMPARE_SCENARIOS="${buffer_scenarios}" \
  SMEAR_COMPARE_REPORT_FILE="${adaptive_report}" \
  "${script_dir}/compare_buffer_perf_modes.sh"
echo

echo "== planner compile =="
SMEAR_COMPARE_REPEATS="${repeats}" \
  SMEAR_COMPARE_PLANNER_MODES="${planner_modes}" \
  SMEAR_COMPARE_SCENARIOS="${planner_scenarios}" \
  SMEAR_COMPARE_REPORT_FILE="${planner_report}" \
  "${script_dir}/compare_planner_perf.sh"
echo

echo "== particle toggle =="
SMEAR_COMPARE_REPEATS="${particle_repeats}" \
  SMEAR_COMPARE_REPORT_FILE="${particle_report}" \
  "${script_dir}/compare_particle_toggle_perf.sh" "${base_ref}"
echo

echo "== window pool cap =="
SMEAR_COMPARE_REPEATS="${repeats}" \
  SMEAR_COMPARE_SCENARIOS="${cap_scenarios}" \
  SMEAR_COMPARE_REPORT_FILE="${window_cap_report}" \
  SMEAR_COMPARE_REPORT_COMMAND="${canonical_command}" \
  "${script_dir}/compare_window_pool_cap_perf.sh" "${base_ref}"
echo

captured_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
repo_commit="$(git -C "${workspace_dir}" rev-parse HEAD)"
tree_state="$(working_tree_state)"
nvim_version="$("${NVIM_BIN:-nvim}" --version | head -n 1)"

{
  printf '# Smear Cursor Perf Snapshot\n\n'
  printf -- '- Captured (UTC): %s\n' "${captured_at}"
  printf -- '- Repo commit: `%s`\n' "${repo_commit}"
  printf -- '- Working tree: `%s`\n' "${tree_state}"
  printf -- '- Neovim: `%s`\n' "${nvim_version}"
  printf -- '- Base ref: `%s`\n' "${base_ref}"
  printf -- '- Command: `%s`\n' "${canonical_command}"
  printf -- '- Artifacts: `%s`\n' "${artifact_dir}"
  printf -- '- Config: repeats=`%s`, particle_repeats=`%s`, buffer_modes=`%s`, buffer_scenarios=`%s`, planner_modes=`%s`, planner_scenarios=`%s`, cap_scenarios=`%s`\n' \
    "${repeats}" \
    "${particle_repeats}" \
    "${buffer_modes}" \
    "${buffer_scenarios}" \
    "${planner_modes}" \
    "${planner_scenarios}" \
    "${cap_scenarios}"
  printf '\n'
  printf 'This is the only checked-in smear-cursor perf snapshot. The numbers are local point-in-time measurements, not cross-machine golden thresholds.\n'
  printf '\n'
} >"${report_file}"

append_report_body "Adaptive Buffer Policy" "${adaptive_report}"
append_report_body "Planner Compile" "${planner_report}"
append_report_body "Particle Toggle" "${particle_report}"
append_report_body "Window Pool Cap" "${window_cap_report}"

echo "wrote ${report_file}"

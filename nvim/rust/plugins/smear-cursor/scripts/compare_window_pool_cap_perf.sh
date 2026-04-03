#!/usr/bin/env bash
set -euo pipefail

# Compare the same window-switch scenarios on the same code while only changing
# `max_kept_windows`. By default this runs the current tree against `HEAD` with
# `64` windows on the local side and `384` on the base side.

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
base_ref="${1:-HEAD}"

export SMEAR_COMPARE_SCENARIOS="${SMEAR_COMPARE_SCENARIOS:-large_line_count,long_running_repetition,planner_heavy,extmark_heavy,conceal_heavy}"
export SMEAR_COMPARE_LOCAL_OVERRIDES="${SMEAR_COMPARE_LOCAL_OVERRIDES:-SMEAR_MAX_KEPT_WINDOWS=64}"
export SMEAR_COMPARE_BASE_OVERRIDES="${SMEAR_COMPARE_BASE_OVERRIDES:-SMEAR_MAX_KEPT_WINDOWS=384}"
export SMEAR_COMPARE_REPORT_COMMAND="${SMEAR_COMPARE_REPORT_COMMAND:-SMEAR_COMPARE_REPORT_FILE=plugins/smear-cursor/perf/window-pool-cap-current.md ${script_dir}/compare_window_pool_cap_perf.sh ${base_ref}}"

exec "${script_dir}/compare_window_switch_scenarios.sh" "${base_ref}"

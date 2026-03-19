#!/usr/bin/env bash
set -euo pipefail

# Build the Neovim cdylib and run the headless window-switch perf harness.
# Override parameters with env vars, for example:
# SMEAR_LINE_COUNT=50000 SMEAR_BETWEEN_BUFFERS=true SMEAR_STRESS_ITERATIONS=50000 \
# SMEAR_MAX_RECOVERY_RATIO=1.35 SMEAR_MAX_STRESS_RATIO=1.8 \
# SMEAR_RECOVERY_MODE=cold SMEAR_COLD_WAIT_TIMEOUT_MS=2500 \
# SMEAR_LOGGING_LEVEL=4 \
# SMEAR_SCENARIO_SET=single \
# SMEAR_DRAIN_EVERY=16 SMEAR_DELAY_EVENT_TO_SMEAR=1 \
# scripts/run_perf_window_switch.sh
# Logging note: the harness defaults to `logging_level = 4`, and this plugin treats `4` as the
# least verbose setting rather than the most verbose one.

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd -- "${script_dir}/.." && pwd)"
perf_lib="${script_dir}/lib/perf_env.sh"

if [[ ! -f "${perf_lib}" ]]; then
  echo "missing perf helper: ${perf_lib}" >&2
  exit 1
fi
# shellcheck source=plugins/smear_cursor/scripts/lib/perf_env.sh
source "${perf_lib}"

cd "${repo_dir}"

if ! smear_export_runtime_paths "${repo_dir}" >/dev/null; then
  echo "failed to resolve cargo target_directory" >&2
  exit 1
fi
target_directory="${SMEAR_CURSOR_TARGET_DIRECTORY:-}"
if [[ -z "${target_directory}" ]]; then
  echo "failed to resolve cargo target_directory" >&2
  exit 1
fi

cargo build --release

nvim_bin="${NVIM_BIN:-nvim}"
harness_args=(
  "${nvim_bin}"
  --headless
  -u
  NONE
  -c
  "luafile ${repo_dir}/scripts/perf_window_switch.lua"
)

run_perf_scenario() {
  local scenario_name="$1"
  shift

  printf 'PERF_SCENARIO_START name=%s\n' "${scenario_name}"
  env SMEAR_SCENARIO_NAME="${scenario_name}" "$@" "${harness_args[@]}"
  printf 'PERF_SCENARIO_DONE name=%s\n' "${scenario_name}"
}

scenario_set="${SMEAR_SCENARIO_SET:-matrix}"
if [[ "${scenario_set}" == "single" ]]; then
  run_perf_scenario "${SMEAR_SCENARIO_NAME:-single}"
  exit 0
fi

if [[ "${scenario_set}" != "matrix" ]]; then
  echo "unknown SMEAR_SCENARIO_SET: ${scenario_set}" >&2
  exit 1
fi

heavy_burst_stress_iterations="${SMEAR_HEAVY_BURST_STRESS_ITERATIONS:-${SMEAR_STRESS_ITERATIONS:-20000}}"
heavy_burst_stress_rounds="${SMEAR_HEAVY_BURST_STRESS_ROUNDS:-${SMEAR_STRESS_ROUNDS:-4}}"
short_settle_wait_ms="${SMEAR_SHORT_SETTLE_WAIT_MS:-1200}"
long_settle_wait_ms="${SMEAR_LONG_SETTLE_WAIT_MS:-3500}"
cold_wait_timeout_ms="${SMEAR_COLD_WAIT_TIMEOUT_MS:-2500}"
delay_event_to_smear_on="${SMEAR_DELAY_EVENT_TO_SMEAR_ON:-1}"
delay_event_to_smear_off="${SMEAR_DELAY_EVENT_TO_SMEAR_OFF:-0}"

run_perf_scenario \
  burst_delay_on_cold \
  SMEAR_STRESS_ITERATIONS="${heavy_burst_stress_iterations}" \
  SMEAR_STRESS_ROUNDS="${heavy_burst_stress_rounds}" \
  SMEAR_RECOVERY_MODE="cold" \
  SMEAR_COLD_WAIT_TIMEOUT_MS="${cold_wait_timeout_ms}" \
  SMEAR_DELAY_EVENT_TO_SMEAR="${delay_event_to_smear_on}"

run_perf_scenario \
  burst_delay_off_cold \
  SMEAR_STRESS_ITERATIONS="${heavy_burst_stress_iterations}" \
  SMEAR_STRESS_ROUNDS="${heavy_burst_stress_rounds}" \
  SMEAR_RECOVERY_MODE="cold" \
  SMEAR_COLD_WAIT_TIMEOUT_MS="${cold_wait_timeout_ms}" \
  SMEAR_DELAY_EVENT_TO_SMEAR="${delay_event_to_smear_off}"

run_perf_scenario \
  burst_delay_on_short \
  SMEAR_STRESS_ITERATIONS="${heavy_burst_stress_iterations}" \
  SMEAR_STRESS_ROUNDS="${heavy_burst_stress_rounds}" \
  SMEAR_RECOVERY_MODE="fixed" \
  SMEAR_SETTLE_WAIT_MS="${short_settle_wait_ms}" \
  SMEAR_DELAY_EVENT_TO_SMEAR="${delay_event_to_smear_on}"

run_perf_scenario \
  burst_delay_off_short \
  SMEAR_STRESS_ITERATIONS="${heavy_burst_stress_iterations}" \
  SMEAR_STRESS_ROUNDS="${heavy_burst_stress_rounds}" \
  SMEAR_RECOVERY_MODE="fixed" \
  SMEAR_SETTLE_WAIT_MS="${short_settle_wait_ms}" \
  SMEAR_DELAY_EVENT_TO_SMEAR="${delay_event_to_smear_off}"

run_perf_scenario \
  burst_delay_on_long \
  SMEAR_STRESS_ITERATIONS="${heavy_burst_stress_iterations}" \
  SMEAR_STRESS_ROUNDS="${heavy_burst_stress_rounds}" \
  SMEAR_RECOVERY_MODE="fixed" \
  SMEAR_SETTLE_WAIT_MS="${long_settle_wait_ms}" \
  SMEAR_DELAY_EVENT_TO_SMEAR="${delay_event_to_smear_on}"

run_perf_scenario \
  burst_delay_off_long \
  SMEAR_STRESS_ITERATIONS="${heavy_burst_stress_iterations}" \
  SMEAR_STRESS_ROUNDS="${heavy_burst_stress_rounds}" \
  SMEAR_RECOVERY_MODE="fixed" \
  SMEAR_SETTLE_WAIT_MS="${long_settle_wait_ms}" \
  SMEAR_DELAY_EVENT_TO_SMEAR="${delay_event_to_smear_off}"

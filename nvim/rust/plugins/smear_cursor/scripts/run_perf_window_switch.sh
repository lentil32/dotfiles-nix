#!/usr/bin/env bash
set -euo pipefail

# Build the Neovim cdylib and run the headless window-switch perf harness.
# Override parameters with env vars, for example:
# SMEAR_LINE_COUNT=50000 SMEAR_BETWEEN_BUFFERS=true SMEAR_STRESS_ITERATIONS=50000 \
# SMEAR_MAX_RECOVERY_RATIO=1.35 SMEAR_MAX_STRESS_RATIO=1.8 \
# SMEAR_DRAIN_EVERY=16 SMEAR_DELAY_EVENT_TO_SMEAR=1 \
# scripts/run_perf_window_switch.sh

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
exec "${nvim_bin}" --headless -u NONE -c "luafile ${repo_dir}/scripts/perf_window_switch.lua"

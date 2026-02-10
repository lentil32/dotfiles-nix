#!/usr/bin/env bash
set -euo pipefail

# Build the Neovim cdylib and run the headless window-switch perf harness.
# Override parameters with env vars, for example:
# SMEAR_BETWEEN_BUFFERS=true SMEAR_STRESS_ITERATIONS=50000 scripts/run_perf_window_switch.sh

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd -- "${script_dir}/.." && pwd)"

cd "${repo_dir}"

target_directory="$(
  cargo metadata --format-version 1 --no-deps \
    | tr -d '\n' \
    | sed -E 's/.*"target_directory":"([^"]+)".*/\1/'
)"

if [[ -z "${target_directory}" ]]; then
  echo "failed to resolve cargo target_directory" >&2
  exit 1
fi

cargo build --release

export SMEAR_CURSOR_RTP="${SMEAR_CURSOR_RTP:-${repo_dir}}"
if [[ -z "${SMEAR_CURSOR_CPATH:-}" ]]; then
  export SMEAR_CURSOR_CPATH="${target_directory}/release/?.dylib;${target_directory}/release/lib?.dylib;${target_directory}/release/?.so;${target_directory}/release/lib?.so"
fi

nvim_bin="${NVIM_BIN:-nvim}"
exec "${nvim_bin}" --headless -u NONE -c "luafile ${repo_dir}/scripts/perf_window_switch.lua"

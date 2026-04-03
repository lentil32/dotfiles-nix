#!/usr/bin/env bash
set -euo pipefail

# Build the smear cursor cdylib and run a headless Neovim regression check for
# the timer bridge and host boundary wiring.

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd -- "${script_dir}/.." && pwd)"
perf_lib="${script_dir}/lib/perf_env.sh"

if [[ ! -f "${perf_lib}" ]]; then
  echo "missing perf helper: ${perf_lib}" >&2
  exit 1
fi
# shellcheck source=smear-cursor/scripts/lib/perf_env.sh
source "${perf_lib}"

cd "${repo_dir}"

if ! smear_export_runtime_paths "${repo_dir}" >/dev/null; then
  echo "failed to resolve cargo target_directory" >&2
  exit 1
fi

cargo build --release

packaged_runtime_dir="$(mktemp -d /tmp/smear_timer_bridge_runtime.XXXXXX)"

resolve_nvim_bin() {
  if [[ -n "${NVIM_BIN:-}" ]]; then
    printf '%s\n' "${NVIM_BIN}"
    return
  fi

  local wrapped_nvim
  wrapped_nvim="$(command -v nvim)"
  if [[ -f "${wrapped_nvim}" ]]; then
    local unwrapped_nvim
    unwrapped_nvim="$(
      sed -n 's|.*"\(/nix/store/[^"]*neovim-unwrapped[^"]*/bin/nvim\)".*|\1|p' "${wrapped_nvim}" \
        | head -n 1
    )"
    if [[ -n "${unwrapped_nvim}" ]]; then
      printf '%s\n' "${unwrapped_nvim}"
      return
    fi
  fi

  printf '%s\n' "${wrapped_nvim}"
}

log_file="$(mktemp /tmp/smear_timer_bridge.XXXXXX.log)"
cleanup() {
  local status=$?
  if [[ ${status} -ne 0 ]]; then
    echo "timer bridge log preserved at ${log_file}" >&2
  else
    rm -f "${log_file}"
  fi
  rm -rf "${packaged_runtime_dir}"
}
trap cleanup EXIT

if ! smear_stage_packaged_runtime "${repo_dir}" "${packaged_runtime_dir}" nvimrs_smear_cursor >/dev/null 2>&1; then
  echo "failed to stage packaged smear cursor runtime" >&2
  exit 1
fi

nvim_bin="$(resolve_nvim_bin)"

SMEAR_CURSOR_RTP="${packaged_runtime_dir}" \
SMEAR_CURSOR_CPATH="${packaged_runtime_dir}/lua/?.so;${packaged_runtime_dir}/lua/lib?.so" \
SMEAR_CURSOR_LOG_FILE="${log_file}" \
  "${nvim_bin}" --headless -u NONE -c "luafile ${repo_dir}/scripts/test_timer_bridge.lua"

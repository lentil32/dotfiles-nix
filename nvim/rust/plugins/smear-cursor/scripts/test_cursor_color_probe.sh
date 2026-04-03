#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd -- "${script_dir}/.." && pwd)"
nvim_bin="${NVIM_BIN:-nvim}"
probe_kind="${1:-}"

case "${probe_kind}" in
  extmarks|treesitter) ;;
  *)
    echo "usage: ${0} <extmarks|treesitter>" >&2
    exit 1
    ;;
esac

SMEAR_CURSOR_RTP="${repo_dir}" \
  "${nvim_bin}" --headless -u NONE -c "luafile ${repo_dir}/scripts/test_cursor_color_probe_${probe_kind}.lua"

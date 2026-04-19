#!/usr/bin/env bash

smear_resolve_target_directory() {
  local plugin_dir="$1"
  (
    cd "${plugin_dir}"
    cargo metadata --format-version 1 --no-deps \
      | tr -d '\n' \
      | sed -E 's/.*"target_directory":"([^"]+)".*/\1/'
  )
}

smear_default_cpath() {
  local target_directory="$1"
  printf '%s/release/?.dylib;%s/release/lib?.dylib;%s/release/?.so;%s/release/lib?.so' \
    "${target_directory}" \
    "${target_directory}" \
    "${target_directory}" \
    "${target_directory}"
}

smear_export_runtime_paths() {
  local plugin_dir="$1"
  local target_directory

  target_directory="$(smear_resolve_target_directory "${plugin_dir}")"
  if [[ -z "${target_directory}" ]]; then
    return 1
  fi

  export SMEAR_CURSOR_TARGET_DIRECTORY="${target_directory}"
  export SMEAR_CURSOR_RTP="${SMEAR_CURSOR_RTP:-${plugin_dir}}"
  if [[ -z "${SMEAR_CURSOR_CPATH:-}" ]]; then
    export SMEAR_CURSOR_CPATH
    SMEAR_CURSOR_CPATH="$(smear_default_cpath "${target_directory}")"
  fi

  printf '%s\n' "${target_directory}"
}

smear_locate_release_library() {
  local target_directory="$1"
  local lib_base="$2"

  if [[ -f "${target_directory}/release/lib${lib_base}.dylib" ]]; then
    printf '%s\n' "${target_directory}/release/lib${lib_base}.dylib"
    return
  fi
  if [[ -f "${target_directory}/release/lib${lib_base}.so" ]]; then
    printf '%s\n' "${target_directory}/release/lib${lib_base}.so"
    return
  fi
  if [[ -f "${target_directory}/release/${lib_base}.dll" ]]; then
    printf '%s\n' "${target_directory}/release/${lib_base}.dll"
    return
  fi

  find "${target_directory}" -type f \
    \( -name "lib${lib_base}.dylib" -o -name "lib${lib_base}.so" -o -name "${lib_base}.dll" \) \
    | head -n 1
}

smear_stage_packaged_runtime() {
  local plugin_dir="$1"
  local staged_dir="$2"
  local lib_base="${3:-nvimrs_smear_cursor}"
  local out_base="${4:-$lib_base}"
  local target_directory
  local runtime_dir
  local built_lib

  target_directory="$(smear_resolve_target_directory "${plugin_dir}")"
  if [[ -z "${target_directory}" ]]; then
    return 1
  fi

  mkdir -p "${staged_dir}/lua"
  for runtime_dir in \
    autoload \
    colors \
    compiler \
    doc \
    ftdetect \
    ftplugin \
    indent \
    keymap \
    lua \
    plugin \
    queries \
    snippets \
    syntax
  do
    if [[ -d "${plugin_dir}/${runtime_dir}" ]]; then
      cp -R "${plugin_dir}/${runtime_dir}" "${staged_dir}/"
    fi
  done

  built_lib="$(smear_locate_release_library "${target_directory}" "${lib_base}")"
  if [[ -z "${built_lib}" ]]; then
    return 1
  fi

  case "${built_lib}" in
    *.dll) cp "${built_lib}" "${staged_dir}/lua/${out_base}.dll" ;;
    *.dylib|*.so) cp "${built_lib}" "${staged_dir}/lua/${out_base}.so" ;;
    *) return 1 ;;
  esac
}

smear_locate_release_executable() {
  local target_directory="$1"
  local bin_name="$2"

  if [[ -x "${target_directory}/release/${bin_name}" ]]; then
    printf '%s\n' "${target_directory}/release/${bin_name}"
    return
  fi
  if [[ -x "${target_directory}/release/${bin_name}.exe" ]]; then
    printf '%s\n' "${target_directory}/release/${bin_name}.exe"
    return
  fi

  find "${target_directory}/release" -maxdepth 1 -type f \
    \( -name "${bin_name}" -o -name "${bin_name}.exe" \) \
    | head -n 1
}

smear_build_release() {
  local plugin_dir="$1"
  (
    cd "${plugin_dir}"
    if [[ -n "${SMEAR_CARGO_FEATURES:-}" ]]; then
      cargo build --release --lib --features "${SMEAR_CARGO_FEATURES}" >/dev/null
    else
      cargo build --release --lib >/dev/null
    fi
  )
}

smear_build_perf_report_tool() {
  local workspace_dir="$1"
  (
    cd "${workspace_dir}"
    cargo build --release -p nvimrs-smear-perf-report >/dev/null
  )
}

smear_perf_report_binary() {
  local workspace_dir="$1"
  local target_directory

  target_directory="$(smear_resolve_target_directory "${workspace_dir}")"
  if [[ -z "${target_directory}" ]]; then
    return 1
  fi

  smear_locate_release_executable "${target_directory}" "nvimrs-smear-perf-report"
}

smear_perf_report_query() {
  local workspace_dir="$1"
  local schema="$2"
  local log_file="$3"
  local report_binary

  shift 3
  report_binary="$(smear_perf_report_binary "${workspace_dir}")"
  if [[ -z "${report_binary}" ]]; then
    return 1
  fi

  "${report_binary}" query "${schema}" "${log_file}" "$@"
}

smear_compare_plugin_dir() {
  local root_dir="$1"
  printf '%s/nvim/rust/plugins/smear-cursor\n' "${root_dir}"
}

smear_compare_release_cpath() {
  local plugin_dir="$1"
  local target_directory

  target_directory="$(smear_resolve_target_directory "${plugin_dir}")"
  if [[ -z "${target_directory}" ]]; then
    return 1
  fi

  smear_default_cpath "${target_directory}"
}

smear_compare_prepare_worktree() {
  local repo_root="$1"
  local base_ref="$2"
  local worktree_prefix="$3"
  local artifact_prefix="$4"
  local worktree_dir
  local artifact_dir

  worktree_dir="$(mktemp -d "/tmp/${worktree_prefix}.XXXXXX")"
  artifact_dir="$(mktemp -d "/tmp/${artifact_prefix}.XXXXXX")"
  git -C "${repo_root}" worktree add --detach "${worktree_dir}" "${base_ref}" >/dev/null

  printf '%s\t%s\n' "${worktree_dir}" "${artifact_dir}"
}

smear_compare_remove_worktree() {
  local repo_root="$1"
  local worktree_dir="$2"

  git -C "${repo_root}" worktree remove "${worktree_dir}" >/dev/null 2>&1 || true
}

smear_report_git_commit() {
  local repo_root="$1"
  git -C "${repo_root}" rev-parse HEAD 2>/dev/null || printf 'unknown\n'
}

smear_report_git_state() {
  local repo_root="$1"

  if [[ -n "$(git -C "${repo_root}" status --short 2>/dev/null)" ]]; then
    printf 'dirty\n'
  else
    printf 'clean\n'
  fi
}

smear_report_nvim_version() {
  "${NVIM_BIN:-nvim}" --version | sed -n '1p'
}

smear_report_capture_time_utc() {
  date -u +%Y-%m-%dT%H:%M:%SZ
}

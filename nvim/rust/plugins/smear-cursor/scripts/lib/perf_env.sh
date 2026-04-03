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

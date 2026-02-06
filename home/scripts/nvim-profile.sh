#!/usr/bin/env bash
set -euo pipefail

print_usage() {
  cat <<'EOF'
Profile Neovim startup latency with hyperfine and preserve diagnostic artifacts.

Usage:
  nvim-profile [options] [target-path]

Options:
  -r, --runs N           Timed runs per scenario (default: 10)
  -u, --warmup N         Warmup runs per scenario (default: 0)
  -m, --mode MODE        tui | headless | both (default: tui)
  -w, --wezterm MODE     auto | current | with | without | both (default: auto)
  -n, --nvim BIN         Neovim binary to execute (default: nvim)
  -o, --out DIR          Output directory (default: ./nvim/artifacts/nvim-profile-<timestamp>)
  -v, --verbose N        Enable Neovim -V logging with level N (default: 0/off)
      --vim-profile      Enable :profile file/function tracing from startup
      --capture-tui      Save TUI I/O transcript per run via `script`
      --clean            Add --clean to Neovim startup
      --no-target        Do not pass a path argument to Neovim
  -h, --help             Show this help

Examples:
  nvim-profile --runs 12 --mode both .
  nvim-profile --wezterm both --runs 8 ~/work/project
  nvim-profile --verbose 3 --capture-tui --runs 5 --mode tui .
  nvim-profile --vim-profile --mode headless --no-target

Notes:
  - Uses hyperfine as the wall-clock benchmarking engine.
  - Each run records Neovim --startuptime for hotspot aggregation.
  - `--verbose` maps to Neovim `-V`.
  - `--vim-profile` enables Neovim built-in :profile output.
  - `--capture-tui` keeps terminal transcript logs that are useful for TUI handshake debugging.
EOF
}

fail() {
  echo "error: $*" >&2
  exit 1
}

require_command() {
  local command_name="$1"
  command -v "$command_name" >/dev/null 2>&1 || fail "required command '$command_name' is not available"
}

is_non_negative_int() {
  [[ "$1" =~ ^[0-9]+$ ]]
}

build_top_self_report() {
  local output_file="$1"
  shift
  local logs=("$@")

  awk '
    match($0, /^([0-9]+\.[0-9]+)[[:space:]]+([0-9]+\.[0-9]+)[[:space:]]+([0-9]+\.[0-9]+): (.*)$/, m) {
      self_time = m[3] + 0.0
      event = m[4]
      total[event] += self_time
      count[event] += 1
    }
    END {
      for (event in total) {
        printf "%.3f\t%.3f\t%d\t%s\n", total[event], total[event] / count[event], count[event], event
      }
    }
  ' "${logs[@]}" | sort -nr -k1,1 | head -n 30 > "$output_file"
}

write_runner_script() {
  local runner_script="$1"
  local logs_dir="$2"
  local log_index="$3"
  local verbose_index="$4"
  local profile_index="$5"
  local tty_index="$6"
  local scenario="$7"
  local mode_name="$8"

  {
    echo '#!/usr/bin/env bash'
    echo 'set -euo pipefail'
    printf 'nvim_bin=%q\n' "$nvim_bin"
    printf 'target_path=%q\n' "$target_path"
    printf 'logs_dir=%q\n' "$logs_dir"
    printf 'log_index=%q\n' "$log_index"
    printf 'verbose_index=%q\n' "$verbose_index"
    printf 'profile_index=%q\n' "$profile_index"
    printf 'tty_index=%q\n' "$tty_index"
    printf 'scenario=%q\n' "$scenario"
    printf 'mode_name=%q\n' "$mode_name"
    printf 'use_clean=%q\n' "$use_clean"
    printf 'no_target=%q\n' "$no_target"
    printf 'wezterm_pane_value=%q\n' "$wezterm_pane_value"
    printf 'verbose_level=%q\n' "$verbose_level"
    printf 'enable_vim_profile=%q\n' "$enable_vim_profile"
    printf 'capture_tui=%q\n' "$capture_tui"
    cat <<'EOF'
command=( "$nvim_bin" )
if [[ "$use_clean" -eq 1 ]]; then
  command+=(--clean)
fi
if [[ "$mode_name" == "headless" ]]; then
  command+=(--headless)
fi

verbose_file=""
if [[ "$verbose_level" -gt 0 ]]; then
  verbose_file="$(mktemp "${logs_dir}/${scenario}-${mode_name}-vim-verbose-XXXXXX.log")"
  command+=("-V${verbose_level}${verbose_file}")
fi

vim_profile_file=""
if [[ "$enable_vim_profile" -eq 1 ]]; then
  vim_profile_file="$(mktemp "${logs_dir}/${scenario}-${mode_name}-vim-profile-XXXXXX.log")"
  command+=(--cmd "profile start ${vim_profile_file}")
  command+=(--cmd "profile file *")
  command+=(--cmd "profile func *")
fi

log_file="$(mktemp "${logs_dir}/${scenario}-${mode_name}-startuptime-XXXXXX.log")"
command+=(--startuptime "$log_file")
if [[ "$no_target" -eq 0 ]]; then
  command+=("$target_path")
fi
command+=(+qa)

tty_log_file=""
script_output_file="/dev/null"
if [[ "$mode_name" == "tui" && "$capture_tui" -eq 1 ]]; then
  tty_log_file="$(mktemp "${logs_dir}/${scenario}-${mode_name}-tty-XXXXXX.log")"
  script_output_file="$tty_log_file"
fi

case "$scenario" in
with_wezterm)
  if [[ "$mode_name" == "tui" ]]; then
    WEZTERM_PANE="$wezterm_pane_value" script -q "$script_output_file" "${command[@]}" >/dev/null 2>&1
  else
    WEZTERM_PANE="$wezterm_pane_value" "${command[@]}" >/dev/null 2>&1
  fi
  ;;
without_wezterm)
  if [[ "$mode_name" == "tui" ]]; then
    env -u WEZTERM_PANE script -q "$script_output_file" "${command[@]}" >/dev/null 2>&1
  else
    env -u WEZTERM_PANE "${command[@]}" >/dev/null 2>&1
  fi
  ;;
current_env)
  if [[ "$mode_name" == "tui" ]]; then
    script -q "$script_output_file" "${command[@]}" >/dev/null 2>&1
  else
    "${command[@]}" >/dev/null 2>&1
  fi
  ;;
*)
  echo "unknown scenario: $scenario" >&2
  exit 1
  ;;
esac

printf '%s\n' "$log_file" >> "$log_index"
if [[ -n "$verbose_file" ]]; then
  printf '%s\n' "$verbose_file" >> "$verbose_index"
fi
if [[ -n "$vim_profile_file" ]]; then
  printf '%s\n' "$vim_profile_file" >> "$profile_index"
fi
if [[ -n "$tty_log_file" ]]; then
  printf '%s\n' "$tty_log_file" >> "$tty_index"
fi
EOF
  } > "$runner_script"
  chmod +x "$runner_script"
}

append_hyperfine_row() {
  local summary_file="$1"
  local scenario="$2"
  local mode_name="$3"
  local json_file="$4"

  local parsed
  parsed="$(
    jq -r '
      .results[0] as $r
      | ($r.times // [] | sort) as $times
      | ($times | length) as $n
      | if $n == 0 then
          empty
        else
          ((($n * 0.95) | ceil) - 1) as $p95_idx
          | [
              $n,
              ($r.mean * 1000),
              ($r.median * 1000),
              ($times[$p95_idx] * 1000),
              ($r.min * 1000),
              ($r.max * 1000),
              (($r.stddev // 0) * 1000)
            ]
          | @tsv
        end
    ' "$json_file"
  )"

  [[ -n "$parsed" ]] || fail "failed to parse hyperfine results from $json_file"

  local measured_runs avg_ms p50_ms p95_ms min_ms max_ms stddev_ms
  IFS=$'\t' read -r measured_runs avg_ms p50_ms p95_ms min_ms max_ms stddev_ms <<< "$parsed"

  printf "%s\t%s\thyperfine\t%s\t%.3f\t%.3f\t%.3f\t%.3f\t%.3f\t%.3f\n" \
    "$scenario" "$mode_name" "$measured_runs" "$avg_ms" "$p50_ms" "$p95_ms" "$min_ms" "$max_ms" "$stddev_ms" >> "$summary_file"
}

runs=10
warmup=0
mode="tui"
wezterm_mode="auto"
nvim_bin="${NVIM_BIN:-nvim}"
target_path="."
target_set=0
use_clean=0
no_target=0
verbose_level=0
enable_vim_profile=0
capture_tui=0
out_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
  -r | --runs)
    [[ $# -ge 2 ]] || fail "$1 expects a value"
    runs="$2"
    shift 2
    ;;
  -u | --warmup)
    [[ $# -ge 2 ]] || fail "$1 expects a value"
    warmup="$2"
    shift 2
    ;;
  -m | --mode)
    [[ $# -ge 2 ]] || fail "$1 expects a value"
    mode="$2"
    shift 2
    ;;
  -w | --wezterm)
    [[ $# -ge 2 ]] || fail "$1 expects a value"
    wezterm_mode="$2"
    shift 2
    ;;
  -n | --nvim)
    [[ $# -ge 2 ]] || fail "$1 expects a value"
    nvim_bin="$2"
    shift 2
    ;;
  -o | --out)
    [[ $# -ge 2 ]] || fail "$1 expects a value"
    out_dir="$2"
    shift 2
    ;;
  -v | --verbose)
    [[ $# -ge 2 ]] || fail "$1 expects a value"
    verbose_level="$2"
    shift 2
    ;;
  --vim-profile)
    enable_vim_profile=1
    shift
    ;;
  --capture-tui)
    capture_tui=1
    shift
    ;;
  --clean)
    use_clean=1
    shift
    ;;
  --no-target)
    no_target=1
    shift
    ;;
  -h | --help)
    print_usage
    exit 0
    ;;
  --)
    shift
    break
    ;;
  -*)
    fail "unknown option '$1'"
    ;;
  *)
    if [[ "$target_set" -eq 1 ]]; then
      fail "only one target path is supported"
    fi
    target_path="$1"
    target_set=1
    shift
    ;;
  esac
done

if [[ $# -gt 0 ]]; then
  fail "unexpected extra arguments: $*"
fi

is_non_negative_int "$runs" || fail "--runs must be a non-negative integer"
is_non_negative_int "$warmup" || fail "--warmup must be a non-negative integer"
is_non_negative_int "$verbose_level" || fail "--verbose must be a non-negative integer"
[[ "$runs" -gt 0 ]] || fail "--runs must be greater than zero"

if [[ "$no_target" -eq 1 && "$target_set" -eq 1 ]]; then
  fail "--no-target cannot be used with an explicit target path"
fi

case "$mode" in
tui | headless | both) ;;
*)
  fail "--mode must be one of: tui, headless, both"
  ;;
esac

case "$wezterm_mode" in
auto | current | with | without | both) ;;
*)
  fail "--wezterm must be one of: auto, current, with, without, both"
  ;;
esac

require_command "$nvim_bin"
require_command "hyperfine"
require_command "jq"
if [[ "$mode" != "headless" ]]; then
  require_command "script"
fi

timestamp=$(date +"%Y%m%d-%H%M%S")
if [[ -z "$out_dir" ]]; then
  out_dir="nvim/artifacts/nvim-profile-${timestamp}"
fi

logs_dir="${out_dir}/logs"
mkdir -p "$logs_dir"

summary_tsv="${out_dir}/summary.tsv"
printf "scenario\tmode\tengine\truns\tavg_ms\tp50_ms\tp95_ms\tmin_ms\tmax_ms\tstddev_ms\n" > "$summary_tsv"

declare -a mode_list
case "$mode" in
tui)
  mode_list=(tui)
  ;;
headless)
  mode_list=(headless)
  ;;
both)
  mode_list=(tui headless)
  ;;
esac

wezterm_pane_value="${WEZTERM_PANE-}"
declare -a scenario_list
case "$wezterm_mode" in
auto)
  if [[ -n "$wezterm_pane_value" ]]; then
    scenario_list=(with_wezterm without_wezterm)
  else
    scenario_list=(current_env)
  fi
  ;;
current)
  scenario_list=(current_env)
  ;;
with)
  [[ -n "$wezterm_pane_value" ]] || fail "--wezterm with requires WEZTERM_PANE to be set"
  scenario_list=(with_wezterm)
  ;;
without)
  scenario_list=(without_wezterm)
  ;;
both)
  if [[ -n "$wezterm_pane_value" ]]; then
    scenario_list=(with_wezterm without_wezterm)
  else
    scenario_list=(without_wezterm)
  fi
  ;;
esac

echo "Profiling Neovim startup"
echo "  engine: hyperfine"
echo "  nvim binary: ${nvim_bin}"
echo "  runs: ${runs}"
echo "  warmup: ${warmup}"
echo "  mode(s): ${mode_list[*]}"
echo "  scenario(s): ${scenario_list[*]}"
echo "  verbose level: ${verbose_level}"
echo "  vim profile: ${enable_vim_profile}"
echo "  capture tui transcript: ${capture_tui}"
if [[ "$no_target" -eq 1 ]]; then
  echo "  target: <none>"
else
  echo "  target: ${target_path}"
fi
echo "  output: ${out_dir}"
echo

for scenario in "${scenario_list[@]}"; do
  for mode_name in "${mode_list[@]}"; do
    scenario_key="${scenario}-${mode_name}"
    log_index="${out_dir}/${scenario_key}-startuptime-logs.txt"
    verbose_index="${out_dir}/${scenario_key}-verbose-logs.txt"
    profile_index="${out_dir}/${scenario_key}-vim-profile-logs.txt"
    tty_index="${out_dir}/${scenario_key}-tty-logs.txt"
    runner_script="${out_dir}/${scenario_key}-runner.sh"
    json_file="${out_dir}/${scenario_key}-hyperfine.json"
    top_file="${out_dir}/${scenario_key}-top-self.tsv"

    : > "$log_index"
    : > "$verbose_index"
    : > "$profile_index"
    : > "$tty_index"

    write_runner_script "$runner_script" "$logs_dir" "$log_index" "$verbose_index" "$profile_index" "$tty_index" "$scenario" "$mode_name"

    echo "Benchmarking ${scenario_key}"
    hyperfine --style basic --shell=none --runs "$runs" --warmup "$warmup" --export-json "$json_file" "$runner_script"

    append_hyperfine_row "$summary_tsv" "$scenario" "$mode_name" "$json_file"

    log_files=()
    while IFS= read -r log_path; do
      [[ -f "$log_path" ]] && log_files+=( "$log_path" )
    done < "$log_index"
    if [[ "${#log_files[@]}" -gt 0 ]]; then
      build_top_self_report "$top_file" "${log_files[@]}"
    fi

    echo
  done
done

echo "Summary (milliseconds):"
if command -v column >/dev/null 2>&1; then
  column -t -s $'\t' "$summary_tsv"
else
  cat "$summary_tsv"
fi
echo
echo "Artifacts:"
echo "  ${summary_tsv}"
echo "  ${logs_dir}"
echo "  ${out_dir}/*-hyperfine.json"
echo "  ${out_dir}/*-startuptime-logs.txt"
echo "  ${out_dir}/*-top-self.tsv"
if [[ "$verbose_level" -gt 0 ]]; then
  echo "  ${out_dir}/*-verbose-logs.txt"
fi
if [[ "$enable_vim_profile" -eq 1 ]]; then
  echo "  ${out_dir}/*-vim-profile-logs.txt"
fi
if [[ "$capture_tui" -eq 1 ]]; then
  echo "  ${out_dir}/*-tty-logs.txt"
fi

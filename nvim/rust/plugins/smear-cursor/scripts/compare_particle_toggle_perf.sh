#!/usr/bin/env bash
set -euo pipefail

# Compare the direct `step` particle path for the local working tree against a base git ref.
# Both cases drive the exact same deterministic trajectory and RNG seed; the only behavior change
# between `particles_off` and `particles_on` is `SMEAR_PARTICLES_ENABLED`.
#
# Usage:
#   scripts/compare_particle_toggle_perf.sh
#   scripts/compare_particle_toggle_perf.sh HEAD~1
#
# Tunables:
#   SMEAR_COMPARE_REPEATS            (default: 3)
#   SMEAR_WARMUP_ITERATIONS          (default: 600)
#   SMEAR_BENCHMARK_ITERATIONS       (default: 2400)
#   SMEAR_RETARGET_INTERVAL          (default: 24)
#   SMEAR_TIME_INTERVAL_MS           (default: 8.333333333333334)
#   SMEAR_PARTICLE_MAX_NUM           (default: 100)
#   SMEAR_COMPARE_REPORT_FILE        (optional; write a markdown snapshot report)

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
rust_repo_dir="$(cd -- "${script_dir}/../../.." && pwd)"
repo_root="$(cd -- "${rust_repo_dir}/../.." && pwd)"
driver_lua="${rust_repo_dir}/plugins/smear-cursor/scripts/perf_particle_toggle.lua"
perf_lib="${script_dir}/lib/perf_env.sh"

if [[ ! -f "${perf_lib}" ]]; then
  echo "missing perf helper: ${perf_lib}" >&2
  exit 1
fi
# shellcheck source=plugins/smear-cursor/scripts/lib/perf_env.sh
source "${perf_lib}"

base_ref="${1:-HEAD}"
repeats="${SMEAR_COMPARE_REPEATS:-3}"
warmup_iterations="${SMEAR_WARMUP_ITERATIONS:-600}"
benchmark_iterations="${SMEAR_BENCHMARK_ITERATIONS:-2400}"
retarget_interval="${SMEAR_RETARGET_INTERVAL:-24}"
time_interval_ms="${SMEAR_TIME_INTERVAL_MS:-8.333333333333334}"
particle_max_num="${SMEAR_PARTICLE_MAX_NUM:-100}"
report_file="${SMEAR_COMPARE_REPORT_FILE:-}"

worktree_dir="$(mktemp -d /tmp/smear_particle_toggle_compare.XXXXXX)"
artifact_dir="$(mktemp -d /tmp/smear_particle_toggle_compare_artifacts.XXXXXX)"
results_tsv="${artifact_dir}/particle_toggle_compare_results.tsv"
raw_results_table="${artifact_dir}/particle_toggle_compare_raw.txt"
summary_table="${artifact_dir}/particle_toggle_compare_summary.txt"
worst_case_table="${artifact_dir}/particle_toggle_compare_worst_case.txt"
particle_isolation_table="${artifact_dir}/particle_toggle_compare_isolation.txt"
delta_table="${artifact_dir}/particle_toggle_compare_delta.txt"

cleanup() {
  git -C "${repo_root}" worktree remove "${worktree_dir}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

git -C "${repo_root}" worktree add --detach "${worktree_dir}" "${base_ref}" >/dev/null

build_release() {
  local plugin_dir="$1"
  (
    cd "${plugin_dir}"
    cargo build --release >/dev/null
  )
}

extract_field() {
  local line="$1"
  local field="$2"

  printf '%s\n' "${line}" | sed -nE "s/.*${field}=([^ ]+).*/\\1/p"
}

run_once() {
  local side_label="$1"
  local side_root="$2"
  local case_label="$3"
  local particles_enabled="$4"
  local repeat_index="$5"
  local plugin_dir="${side_root}/nvim/rust/plugins/smear-cursor"
  local log_file="${artifact_dir}/run_${side_label}_${case_label}_${repeat_index}.log"
  local target_directory
  local smear_cursor_cpath
  local summary_line
  local avg_us
  local avg_particles
  local max_particles
  local final_particles

  target_directory="$(smear_resolve_target_directory "${plugin_dir}")"
  if [[ -z "${target_directory}" ]]; then
    echo "failed to resolve target_directory for ${side_label}" >&2
    exit 1
  fi
  smear_cursor_cpath="$(smear_default_cpath "${target_directory}")"

  (
    cd "${plugin_dir}"
    SMEAR_CURSOR_CPATH="${smear_cursor_cpath}" \
      SMEAR_PARTICLES_ENABLED="${particles_enabled}" \
      SMEAR_WARMUP_ITERATIONS="${warmup_iterations}" \
      SMEAR_BENCHMARK_ITERATIONS="${benchmark_iterations}" \
      SMEAR_RETARGET_INTERVAL="${retarget_interval}" \
      SMEAR_TIME_INTERVAL_MS="${time_interval_ms}" \
      SMEAR_PARTICLE_MAX_NUM="${particle_max_num}" \
      "${NVIM_BIN:-nvim}" --headless -u NONE -c "luafile ${driver_lua}"
  ) >"${log_file}" 2>&1

  summary_line="$(grep 'PERF_SUMMARY' "${log_file}" | tail -n 1)"
  avg_us="$(extract_field "${summary_line}" "avg_us")"
  avg_particles="$(extract_field "${summary_line}" "avg_particles")"
  max_particles="$(extract_field "${summary_line}" "max_particles")"
  final_particles="$(extract_field "${summary_line}" "final_particles")"

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "${side_label}" \
    "${case_label}" \
    "${repeat_index}" \
    "${avg_us}" \
    "${avg_particles}" \
    "${max_particles}" \
    "${final_particles}" \
    >>"${results_tsv}"

  printf '%s %-13s run=%s avg_us=%8s avg_particles=%s max_particles=%s final_particles=%s\n' \
    "${side_label}" \
    "${case_label}" \
    "${repeat_index}" \
    "${avg_us}" \
    "${avg_particles}" \
    "${max_particles}" \
    "${final_particles}"
}

run_side() {
  local side_label="$1"
  local side_root="$2"
  local plugin_dir="${side_root}/nvim/rust/plugins/smear-cursor"
  local repeat_index

  build_release "${plugin_dir}"
  for repeat_index in $(seq 1 "${repeats}"); do
    run_once "${side_label}" "${side_root}" "particles_off" "0" "${repeat_index}"
  done
  for repeat_index in $(seq 1 "${repeats}"); do
    run_once "${side_label}" "${side_root}" "particles_on" "1" "${repeat_index}"
  done
}

render_raw_results() {
  column -t -s $'\t' "${results_tsv}"
}

render_summary_table() {
  {
    printf 'side\tcase\tavg_step_us\tavg_particles\tmax_particles\tavg_final_particles\n'
    for side_label in local base; do
      for case_label in particles_off particles_on; do
        awk -F '\t' -v side="${side_label}" -v case_name="${case_label}" '
          NR == 1 { next }
          $1 == side && $2 == case_name {
            avg_us_sum += $4
            avg_particles_sum += $5
            final_particles_sum += $7
            if ($6 > max_particles) {
              max_particles = $6
            }
            count += 1
          }
          END {
            if (count > 0) {
              printf "%s\t%s\t%.3f\t%.3f\t%d\t%.2f\n",
                side,
                case_name,
                avg_us_sum / count,
                avg_particles_sum / count,
                max_particles,
                final_particles_sum / count
            }
          }
        ' "${results_tsv}"
      done
    done
  } | column -t -s $'\t'
}

render_worst_case_table() {
  {
    printf 'side\tcase\tworst_step_us\tworst_avg_particles\tmax_particles\tworst_final_particles\n'
    for side_label in local base; do
      for case_label in particles_off particles_on; do
        awk -F '\t' -v side="${side_label}" -v case_name="${case_label}" '
          NR == 1 { next }
          $1 == side && $2 == case_name {
            if ($4 > worst_step_us) {
              worst_step_us = $4
            }
            if ($5 > worst_avg_particles) {
              worst_avg_particles = $5
            }
            if ($6 > max_particles) {
              max_particles = $6
            }
            if ($7 > worst_final_particles) {
              worst_final_particles = $7
            }
            count += 1
          }
          END {
            if (count > 0) {
              printf "%s\t%s\t%.3f\t%.3f\t%d\t%d\n",
                side,
                case_name,
                worst_step_us,
                worst_avg_particles,
                max_particles,
                worst_final_particles
            }
          }
        ' "${results_tsv}"
      done
    done
  } | column -t -s $'\t'
}

render_particle_isolation_table() {
  awk -F '\t' '
    NR == 1 { next }
    {
      avg_us_sum[$1 "|" $2] += $4
      avg_particles_sum[$1 "|" $2] += $5
      count[$1 "|" $2] += 1
      if ($6 > max_particles[$1 "|" $2]) {
        max_particles[$1 "|" $2] = $6
      }
    }
    END {
      printf "side\tparticles_off_avg_step_us\tparticles_on_avg_step_us\tparticle_tax_pct\tparticles_on_avg_particles\tparticles_on_max_particles\n"
      for (side_index = 1; side_index <= 2; side_index++) {
        side = side_index == 1 ? "local" : "base"
        off_key = side "|particles_off"
        on_key = side "|particles_on"
        if (count[off_key] == 0 || count[on_key] == 0) {
          continue
        }

        off_avg_us = avg_us_sum[off_key] / count[off_key]
        on_avg_us = avg_us_sum[on_key] / count[on_key]
        particle_tax_pct = (on_avg_us - off_avg_us) / off_avg_us * 100.0
        on_avg_particles = avg_particles_sum[on_key] / count[on_key]

        printf "%s\t%.3f\t%.3f\t%+.2f%%\t%.3f\t%d\n",
          side,
          off_avg_us,
          on_avg_us,
          particle_tax_pct,
          on_avg_particles,
          max_particles[on_key]
      }
    }
  ' "${results_tsv}" | column -t -s $'\t'
}

render_delta_table() {
  {
    printf 'case\tlocal_avg_step_us\tbase_avg_step_us\tdelta_pct\tlocal_avg_particles\tbase_avg_particles\n'
    for case_label in particles_off particles_on; do
      awk -F '\t' -v case_name="${case_label}" '
        NR == 1 { next }
        $2 == case_name {
          avg_us_sum[$1] += $4
          avg_particles_sum[$1] += $5
          count[$1] += 1
        }
        END {
          local_avg_us = avg_us_sum["local"] / count["local"]
          base_avg_us = avg_us_sum["base"] / count["base"]
          local_avg_particles = avg_particles_sum["local"] / count["local"]
          base_avg_particles = avg_particles_sum["base"] / count["base"]
          delta_pct = (local_avg_us - base_avg_us) / base_avg_us * 100.0
          printf "%s\t%.3f\t%.3f\t%+.2f%%\t%.3f\t%.3f\n",
            case_name,
            local_avg_us,
            base_avg_us,
            delta_pct,
            local_avg_particles,
            base_avg_particles
        }
      ' "${results_tsv}"
    done
  } | column -t -s $'\t'
}

write_report() {
  local output_file="$1"
  local git_commit
  local git_state
  local nvim_version
  local capture_time
  local command_line

  git_commit="$(git -C "${repo_root}" rev-parse HEAD 2>/dev/null || printf 'unknown\n')"
  if [[ -n "$(git -C "${repo_root}" status --short 2>/dev/null)" ]]; then
    git_state="dirty"
  else
    git_state="clean"
  fi
  nvim_version="$("${NVIM_BIN:-nvim}" --version | sed -n '1p')"
  capture_time="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  command_line="SMEAR_COMPARE_REPORT_FILE=${output_file} ${rust_repo_dir}/plugins/smear-cursor/scripts/compare_particle_toggle_perf.sh ${base_ref}"

  mkdir -p "$(dirname -- "${output_file}")"
  {
    printf '# Particle Toggle Perf Snapshot\n\n'
    printf -- '- Captured (UTC): %s\n' "${capture_time}"
    printf -- '- Repo commit: `%s`\n' "${git_commit}"
    printf -- '- Working tree: `%s`\n' "${git_state}"
    printf -- '- Neovim: `%s`\n' "${nvim_version}"
    printf -- '- Base ref: `%s`\n' "${base_ref}"
    printf -- '- Command: `%s`\n' "${command_line}"
    printf -- '- Config: repeats=`%s`, warmup=`%s`, benchmark=`%s`, retarget_interval=`%s`, time_interval_ms=`%s`, particle_max_num=`%s`\n' \
      "${repeats}" \
      "${warmup_iterations}" \
      "${benchmark_iterations}" \
      "${retarget_interval}" \
      "${time_interval_ms}" \
      "${particle_max_num}"
    printf '\nThis benchmark drives the same deterministic `smear.step()` trajectory for both cases and only flips `SMEAR_PARTICLES_ENABLED`.\n'
    printf '\n## Raw Results\n\n```text\n'
    cat "${raw_results_table}"
    printf '```\n\n## Summary\n\n```text\n'
    cat "${summary_table}"
    printf '```\n\n## Worst-Case Repeats\n\n```text\n'
    cat "${worst_case_table}"
    printf '```\n\n## Particle Isolation (same side)\n\n```text\n'
    cat "${particle_isolation_table}"
    printf '```\n\n## Delta (local vs base)\n\n```text\n'
    cat "${delta_table}"
    printf '```\n'
  } >"${output_file}"
}

printf 'side\tcase\trun\tavg_us\tavg_particles\tmax_particles\tfinal_particles\n' >"${results_tsv}"

echo "base_ref=${base_ref}"
echo "local_root=${repo_root}"
echo "base_root=${worktree_dir}"
echo "artifacts=${artifact_dir}"
echo

run_side "local" "${repo_root}"
run_side "base" "${worktree_dir}"

echo
echo "== Raw Results =="
render_raw_results | tee "${raw_results_table}"

echo
echo "== Summary =="
render_summary_table | tee "${summary_table}"

echo
echo "== Worst-Case Repeats =="
render_worst_case_table | tee "${worst_case_table}"

echo
echo "== Particle Isolation (same side) =="
render_particle_isolation_table | tee "${particle_isolation_table}"

echo
echo "== Delta (local vs base) =="
render_delta_table | tee "${delta_table}"

if [[ -n "${report_file}" ]]; then
  write_report "${report_file}"
  echo
  echo "report=${report_file}"
fi

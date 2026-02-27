#!/usr/bin/env bash
set -euo pipefail

# Pinpoint active-render regressions by replaying targeted file-group reverts from a base ref.
# Default base ref is HEAD~1 (the immediate parent commit).
#
# Usage:
#   scripts/pinpoint_active_regression.sh
#   scripts/pinpoint_active_regression.sh <base-ref>
#
# Output:
#   - per-variant, per-case metrics
#   - summary table with average baseline deltas vs current
#   - best-improving variant (top suspect)

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
rust_repo_dir="$(cd -- "${script_dir}/../.." && pwd)"
repo_root="$(cd -- "${rust_repo_dir}/../.." && pwd)"
perf_lib="${script_dir}/lib/perf_env.sh"

if [[ ! -f "${perf_lib}" ]]; then
  echo "missing perf helper: ${perf_lib}" >&2
  exit 1
fi
# shellcheck source=plugins/smear_cursor/scripts/lib/perf_env.sh
source "${perf_lib}"

base_ref="${1:-HEAD~1}"
worktree_dir="$(mktemp -d /tmp/smear_pinpoint.XXXXXX)"
artifact_dir="$(mktemp -d /tmp/smear_pinpoint_artifacts.XXXXXX)"
results_tsv="${artifact_dir}/pinpoint_results.tsv"

cleanup() {
  # Best-effort cleanup of the temporary worktree.
  git -C "${repo_root}" worktree remove "${worktree_dir}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

git -C "${repo_root}" worktree add --detach "${worktree_dir}" HEAD >/dev/null

variant_files() {
  local variant="$1"
  case "${variant}" in
    current)
      ;;
    base_full)
      cat <<'EOF'
nvim/rust/plugins/smear_cursor/src
nvim/rust/plugins/smear_cursor/Cargo.toml
EOF
      ;;
    frame_static)
      cat <<'EOF'
nvim/rust/plugins/smear_cursor/src/types.rs
nvim/rust/plugins/smear_cursor/src/state.rs
nvim/rust/plugins/smear_cursor/src/core/runtime_reducer.rs
nvim/rust/plugins/smear_cursor/src/draw/render/particles.rs
EOF
      ;;
    animation)
      cat <<'EOF'
nvim/rust/plugins/smear_cursor/src/animation.rs
EOF
      ;;
    geometry)
      cat <<'EOF'
nvim/rust/plugins/smear_cursor/src/draw/render/geometry.rs
nvim/rust/plugins/smear_cursor/src/draw/render/cell_draw.rs
EOF
      ;;
    probe_plan)
      cat <<'EOF'
nvim/rust/plugins/smear_cursor/src/draw/render_plan.rs
nvim/rust/plugins/smear_cursor/src/draw/apply.rs
nvim/rust/plugins/smear_cursor/src/draw/mod.rs
EOF
      ;;
    *)
      echo "unknown variant: ${variant}" >&2
      return 1
      ;;
  esac
}

run_case() {
  local variant="$1"
  local case_label="$2"
  local worktree="$3"
  local keys_per_switch="$4"
  local drain_every="$5"
  local delay_event_to_smear="$6"
  local delay_after_key="$7"
  local smear_between_buffers="$8"

  local log_file="${artifact_dir}/run_${variant}_${case_label}.log"
  (
    cd "${worktree}/nvim/rust"
    SMEAR_WARMUP_ITERATIONS=200 \
      SMEAR_BASELINE_ITERATIONS=1000 \
      SMEAR_STRESS_ITERATIONS=4000 \
      SMEAR_STRESS_ROUNDS=2 \
      SMEAR_RECOVERY_ITERATIONS=1000 \
      SMEAR_SETTLE_WAIT_MS=500 \
      SMEAR_BETWEEN_BUFFERS="${smear_between_buffers}" \
      SMEAR_KEYS_PER_SWITCH="${keys_per_switch}" \
      SMEAR_DRAIN_EVERY="${drain_every}" \
      SMEAR_DELAY_EVENT_TO_SMEAR="${delay_event_to_smear}" \
      SMEAR_DELAY_AFTER_KEY="${delay_after_key}" \
      plugins/smear_cursor/scripts/run_perf_window_switch.sh
  ) >"${log_file}" 2>&1

  local summary_line
  local stress_line
  summary_line="$(grep 'PERF_SUMMARY' "${log_file}" | tail -n 1)"
  stress_line="$(grep 'PERF_STRESS_SUMMARY' "${log_file}" | tail -n 1)"

  local baseline
  local recovery
  local recovery_ratio
  local stress_max
  local stress_ratio
  baseline="$(printf '%s\n' "${summary_line}" | sed -E 's/.*baseline_avg_us=([0-9.]+).*/\1/')"
  recovery="$(printf '%s\n' "${summary_line}" | sed -E 's/.*recovery_avg_us=([0-9.]+).*/\1/')"
  recovery_ratio="$(printf '%s\n' "${summary_line}" | sed -E 's/.*recovery_ratio=([0-9.]+).*/\1/')"
  stress_max="$(printf '%s\n' "${stress_line}" | sed -E 's/.*max_avg_us=([0-9.]+).*/\1/')"
  stress_ratio="$(printf '%s\n' "${stress_line}" | sed -E 's/.*max_ratio=([0-9.]+).*/\1/')"

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "${variant}" "${case_label}" "${baseline}" "${recovery}" "${recovery_ratio}" "${stress_max}" "${stress_ratio}" \
    >>"${results_tsv}"

  printf '%s %-15s baseline=%8sus recovery=%8sus stress_max=%8sus rec_ratio=%s stress_ratio=%s\n' \
    "${variant}" "${case_label}" "${baseline}" "${recovery}" "${stress_max}" "${recovery_ratio}" "${stress_ratio}"
}

run_variant() {
  local variant="$1"
  local worktree="$2"
  local files

  git -C "${worktree}" reset --hard HEAD >/dev/null
  git -C "${worktree}" clean -fd >/dev/null

  files="$(variant_files "${variant}" || true)"
  if [[ -n "${files}" ]]; then
    # Surprising if this fails: the base ref should contain these paths.
    git -C "${worktree}" checkout "${base_ref}" -- ${files}
  fi

  run_case "${variant}" "active_buf_off" "${worktree}" 2 1 0 0 false
  run_case "${variant}" "active_buf_on" "${worktree}" 2 1 0 0 true
}

printf 'variant\tcase\tbaseline_us\trecovery_us\trecovery_ratio\tstress_max_us\tstress_ratio\n' >"${results_tsv}"

variants=(
  current
  base_full
  frame_static
  animation
  geometry
  probe_plan
)

echo "base_ref=${base_ref}"
echo "worktree=${worktree_dir}"
echo

for variant in "${variants[@]}"; do
  run_variant "${variant}" "${worktree_dir}"
done

echo
echo "== Raw Results =="
column -t -s $'\t' "${results_tsv}"

echo
echo "== Variant Summary (avg baseline across active cases) =="
awk -F '\t' '
  NR == 1 { next }
  {
    key = $1
    baseline_sum[key] += $3
    count[key] += 1
  }
  END {
    current_avg = baseline_sum["current"] / count["current"]
    printf "variant\tavg_baseline_us\tdelta_vs_current_pct\n"
    for (variant in baseline_sum) {
      avg = baseline_sum[variant] / count[variant]
      delta = (avg - current_avg) / current_avg * 100.0
      printf "%s\t%.3f\t%+.2f%%\n", variant, avg, delta
    }
  }
' "${results_tsv}" | column -t -s $'\t'

echo
echo "== Top Suspect =="
awk -F '\t' '
  NR == 1 { next }
  {
    key = $1
    baseline_sum[key] += $3
    count[key] += 1
  }
  END {
    current_avg = baseline_sum["current"] / count["current"]
    best_variant = ""
    best_delta = 0.0
    for (variant in baseline_sum) {
      if (variant == "current") {
        continue
      }
      avg = baseline_sum[variant] / count[variant]
      delta = (avg - current_avg) / current_avg * 100.0
      if (best_variant == "" || delta < best_delta) {
        best_variant = variant
        best_delta = delta
      }
    }
    if (best_variant == "") {
      printf "No alternate variant data found.\n"
      exit 0
    }
    printf "%s (avg baseline delta vs current: %+.2f%%)\n", best_variant, best_delta
  }
' "${results_tsv}"

echo
echo "results_tsv=${results_tsv}"
echo "logs_dir=${artifact_dir}"

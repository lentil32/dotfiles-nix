use super::*;

mod fallback;
mod ribbon_dp;
mod ribbon_state_space;
mod staging;

#[cfg(test)]
pub(super) use self::fallback::active_support_is_disconnected;
#[cfg(test)]
pub(super) use self::fallback::decode_compiled_field;
#[cfg(test)]
pub(super) use self::fallback::decode_compiled_field_trace;
pub(super) use self::fallback::decode_compiled_field_trace_with_compiled_and_scratch;
#[cfg(test)]
pub(super) use self::fallback::decode_compiled_field_with_solver;
#[cfg(test)]
pub(super) use self::fallback::ribbon_support_is_oversized;
#[cfg(test)]
pub(super) use self::fallback::select_decode_path;
#[cfg(test)]
pub(super) use self::fallback::solve_pairwise_fallback;
#[cfg(test)]
pub(super) use self::fallback::solve_pairwise_fallback_with_scratch;
#[cfg(test)]
pub(super) use self::ribbon_dp::merge_ribbon_assignments;
#[cfg(test)]
pub(super) use self::ribbon_dp::overlap_penalty;
#[cfg(test)]
pub(super) use self::ribbon_dp::solve_ribbon_dp;
#[cfg(test)]
pub(super) use self::ribbon_dp::transition_cost;
#[cfg(test)]
pub(super) use self::ribbon_state_space::RunEnumerationCursor;
#[cfg(test)]
pub(super) use self::ribbon_state_space::RunEnumerationInput;
#[cfg(test)]
pub(super) use self::ribbon_state_space::adjusted_candidate_cost;
#[cfg(test)]
pub(super) use self::ribbon_state_space::build_slice_states;
#[cfg(test)]
pub(super) use self::ribbon_state_space::build_slice_states_with_peak_working_set;
#[cfg(test)]
pub(super) use self::ribbon_state_space::run_width_cells;
#[cfg(test)]
pub(super) use self::ribbon_state_space::slice_peak_highlight_level;
#[cfg(test)]
pub(super) use self::ribbon_state_space::slice_state_cmp;
#[cfg(test)]
pub(super) use self::ribbon_state_space::state_for_slice_cell;
#[cfg(test)]
pub(super) use self::ribbon_state_space::state_local_prior;
pub(super) use self::staging::aspect_metric_distance;
pub(super) use self::staging::push_decoded_cell;
pub(super) use self::staging::sanitize_spatial_weight_q10;
pub(super) use self::staging::sanitize_temporal_weight;
pub(super) use self::staging::sanitize_top_k;
pub(super) use self::staging::stage_deposited_samples;

pub(super) fn scale_penalty(base: u64, spatial_weight_q10: u32) -> u64 {
    base.saturating_mul(u64::from(spatial_weight_q10)) / 1024
}

pub(super) fn linear_cells_penalty(delta_cells: f64, weight: u64) -> u64 {
    if !delta_cells.is_finite() || delta_cells <= 0.0 {
        return 0;
    }
    let delta_q10 = (delta_cells * WIDTH_Q10_SCALE_U64 as f64)
        .round()
        .clamp(0.0, u64::MAX as f64) as u64;
    let weighted = u128::from(delta_q10).saturating_mul(u128::from(weight));
    (weighted / u128::from(WIDTH_Q10_SCALE_U64)).min(u128::from(u64::MAX)) as u64
}

pub(super) fn squared_cells_penalty(delta_cells: f64, weight: u64) -> u64 {
    if !delta_cells.is_finite() || delta_cells <= 0.0 {
        return 0;
    }
    let delta_q10 = (delta_cells * WIDTH_Q10_SCALE_U64 as f64)
        .round()
        .clamp(0.0, u64::MAX as f64) as u64;
    let weighted = u128::from(delta_q10)
        .saturating_mul(u128::from(delta_q10))
        .saturating_mul(u128::from(weight));
    let denom = u128::from(WIDTH_Q10_SCALE_U64).saturating_mul(u128::from(WIDTH_Q10_SCALE_U64));
    (weighted / denom).min(u128::from(u64::MAX)) as u64
}

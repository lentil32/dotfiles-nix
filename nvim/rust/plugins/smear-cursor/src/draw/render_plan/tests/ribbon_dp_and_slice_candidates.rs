use super::*;
use crate::test_support::proptest::pure_config;
use proptest::collection::vec;
use proptest::prelude::*;

type GeneratedSliceSpec = (u8, Vec<(u16, Vec<u16>)>, u8);

fn transition_cost_linear_baseline(
    previous_slice: &RibbonSlice,
    previous_state: SliceState,
    next_slice: &RibbonSlice,
    next_state: SliceState,
    spatial_weight_q10: u32,
) -> u64 {
    let mut cost = 0_u64;
    for (next_index, next_cell) in next_slice.cells.iter().enumerate() {
        let next_value = state_for_slice_cell(next_slice, next_state, next_index);
        if let Some(previous_index) = previous_slice
            .cells
            .iter()
            .position(|previous_cell| previous_cell.coord == next_cell.coord)
        {
            let previous_value =
                state_for_slice_cell(previous_slice, previous_state, previous_index);
            cost = cost.saturating_add(scale_penalty(
                overlap_penalty(previous_value, next_value),
                spatial_weight_q10,
            ));
        }
    }

    let prev_width = run_width_cells(previous_slice, previous_state);
    let next_width = run_width_cells(next_slice, next_state);
    cost = cost.saturating_add(scale_penalty(
        linear_cells_penalty((prev_width - next_width).abs(), PENALTY_THICKNESS_DELTA),
        spatial_weight_q10,
    ));
    let (headward_len, tailward_len) = if previous_slice.tail_u <= next_slice.tail_u {
        (prev_width, next_width)
    } else {
        (next_width, prev_width)
    };
    let mono_violation = (tailward_len - headward_len - COMET_MONO_EPSILON_CELLS).max(0.0);
    cost = cost.saturating_add(scale_penalty(
        squared_cells_penalty(mono_violation, COMET_MONO_WEIGHT),
        spatial_weight_q10,
    ));

    match (
        previous_slice
            .run_projected_span_q16(previous_state)
            .map(ProjectedSpanQ16::center_q16),
        next_slice
            .run_projected_span_q16(next_state)
            .map(ProjectedSpanQ16::center_q16),
    ) {
        (Some(prev_center), Some(next_center)) => {
            let shift_q16 = prev_center.abs_diff(next_center);
            let shift_penalty =
                (u64::from(shift_q16).saturating_mul(PENALTY_CENTER_SHIFT)) / Q16_SCALE_U64;
            cost = cost.saturating_add(scale_penalty(shift_penalty, spatial_weight_q10));
            if shift_q16 > ((3 * Q16_SCALE) / 2) as u32 {
                cost = cost.saturating_add(scale_penalty(PENALTY_DISCONNECT, spatial_weight_q10));
            }
        }
        (None, Some(_)) | (Some(_), None) => {
            cost = cost.saturating_add(scale_penalty(PENALTY_EMPTY_TRANSITION, spatial_weight_q10));
        }
        (None, None) => {}
    }

    cost
}

fn generated_state(seed: usize) -> DecodedCellState {
    let glyph = match seed % 3 {
        0 => DecodedGlyph::Block,
        1 => DecodedGlyph::Matrix(1 + (seed % 15) as u8),
        _ => DecodedGlyph::Octant(1 + (seed % 255) as u8),
    };
    let level = HighlightLevel::from_raw_clamped(1 + (seed % 16) as u32);

    DecodedCellState { glyph, level }
}

fn slice_from_generated_spec(
    (base_col, cells, target_width_tenths): &GeneratedSliceSpec,
    slice_index: usize,
    slice_count: usize,
) -> RibbonSlice {
    let center = (cells.len().saturating_sub(1) as f64) / 2.0;
    let tail_u = if slice_count <= 1 {
        0.0
    } else {
        1.0 - slice_index as f64 / (slice_count - 1) as f64
    };
    let cells = cells
        .iter()
        .enumerate()
        .map(|(cell_index, (empty_cost, candidate_costs))| {
            let non_empty_candidates = ordered_candidates(
                candidate_costs
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(candidate_index, unary_cost)| CellCandidate {
                        state: Some(generated_state(
                            slice_index * 97 + cell_index * 17 + candidate_index,
                        )),
                        unary_cost: u64::from(unary_cost),
                    })
                    .collect::<Vec<_>>(),
            );

            slice_cell_with_candidates(
                (10, 10 + i64::from(*base_col) + cell_index as i64),
                cell_index as f64 - center,
                0.5,
                u64::from(*empty_cost),
                non_empty_candidates,
            )
        })
        .collect::<Vec<_>>();

    RibbonSlice {
        cells,
        tail_u,
        target_width_cells: 1.0 + f64::from(*target_width_tenths) / 10.0,
        tip_width_cap_cells: COMET_MIN_RESOLVABLE_WIDTH,
        transverse_width_penalty: 0.0,
    }
}

fn small_slice_sequence_strategy() -> BoxedStrategy<Vec<GeneratedSliceSpec>> {
    vec(
        (
            0_u8..=2_u8,
            vec((0_u16..=600_u16, vec(0_u16..=200_u16, 1..=2)), 1..=3),
            0_u8..=20_u8,
        ),
        2..=3,
    )
    .boxed()
}

fn reference_solve_ribbon_dp(
    slices: &[RibbonSlice],
    spatial_weight_q10: u32,
) -> Option<Vec<SliceState>> {
    if slices.len() < 2 {
        return None;
    }

    let state_sets = slices
        .iter()
        .map(|slice| build_slice_states_reference(slice, spatial_weight_q10))
        .collect::<Vec<_>>();
    if state_sets.iter().any(Vec::is_empty) {
        return None;
    }

    let mut costs = vec![
        state_sets[0]
            .iter()
            .map(|state| state.local_cost)
            .collect::<Vec<_>>(),
    ];
    let mut backpointers = vec![vec![0; state_sets[0].len()]];

    for slice_index in 1..state_sets.len() {
        let current_states = &state_sets[slice_index];
        let previous_states = &state_sets[slice_index - 1];
        let mut current_costs = vec![u64::MAX; current_states.len()];
        let mut current_back = vec![0_usize; current_states.len()];

        for (current_index, current_state) in current_states.iter().copied().enumerate() {
            let mut best_prev_index = 0_usize;
            let mut best_cost = u64::MAX;
            for (previous_index, previous_state) in previous_states.iter().copied().enumerate() {
                let transition = transition_cost_linear_baseline(
                    &slices[slice_index - 1],
                    previous_state,
                    &slices[slice_index],
                    current_state,
                    spatial_weight_q10,
                );
                let candidate = costs[slice_index - 1][previous_index]
                    .saturating_add(current_state.local_cost)
                    .saturating_add(transition);
                let should_replace = candidate < best_cost
                    || (candidate == best_cost && previous_index < best_prev_index)
                    || (candidate == best_cost
                        && previous_index == best_prev_index
                        && previous_state.tie_break_key()
                            < previous_states[best_prev_index].tie_break_key());
                if should_replace {
                    best_cost = candidate;
                    best_prev_index = previous_index;
                }
            }
            current_costs[current_index] = best_cost;
            current_back[current_index] = best_prev_index;
        }

        costs.push(current_costs);
        backpointers.push(current_back);
    }

    let last_index = state_sets.len() - 1;
    let mut best_state_index = 0_usize;
    let mut best_total = u64::MAX;
    for (index, total_cost) in costs[last_index].iter().copied().enumerate() {
        let should_replace = total_cost < best_total
            || (total_cost == best_total
                && state_sets[last_index][index].tie_break_key()
                    < state_sets[last_index][best_state_index].tie_break_key())
            || (total_cost == best_total
                && state_sets[last_index][index].tie_break_key()
                    == state_sets[last_index][best_state_index].tie_break_key()
                && index < best_state_index);
        if should_replace {
            best_total = total_cost;
            best_state_index = index;
        }
    }

    let mut path = vec![SliceState::empty(0); state_sets.len()];
    let mut cursor = best_state_index;
    for slice_index in (0..state_sets.len()).rev() {
        path[slice_index] = state_sets[slice_index][cursor];
        if slice_index > 0 {
            cursor = backpointers[slice_index][cursor];
        }
    }

    Some(path)
}

#[test]
fn shade_profile_neighbors_reduce_frame_to_frame_toggling() {
    let age = AgeMoment {
        total_mass_q12: 4095,
        recent_mass_q12: 0,
    };
    let boundary_trace = [2174_u16, 2176_u16, 2174_u16, 2176_u16];

    let decode_trace = |temporal_stability_weight: f64| {
        let mut previous_cells = BTreeMap::<(i64, i64), DecodedCellState>::new();
        let mut decoded_states = Vec::<DecodedCellState>::new();

        for sample_q12 in boundary_trace {
            let compiled = compiled_single_cell(tile_for_octant(1, sample_q12), age);
            let candidates =
                build_cell_candidates(&compiled, &previous_cells, 16, temporal_stability_weight, 5);
            let decoded = decode_locally(&candidates);
            let state = decoded
                .get(&(10_i64, 10_i64))
                .copied()
                .expect("single-cell trace should decode");
            decoded_states.push(state);
            previous_cells = decoded;
        }

        decoded_states
    };

    let unstable = decode_trace(0.0);
    let stable = decode_trace(3.0);

    assert!(
        count_state_toggles(&stable) < count_state_toggles(&unstable),
        "neighbor shade profiles should let temporal stability suppress boundary flicker: unstable={unstable:?} stable={stable:?}"
    );
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_nearest_shade_lookup_matches_linear_baseline(
        color_levels in 1_u32..=64_u32,
        alpha_q12 in 0_u16..=4095_u16,
    ) {
        let shades = build_shade_profiles(color_levels);
        let expected = shades
            .iter()
            .enumerate()
            .min_by_key(|(_, shade)| shade.sample_q12.abs_diff(alpha_q12))
            .map(|(index, _)| index);

        prop_assert_eq!(nearest_shade_profile_index(&shades, alpha_q12), expected);
    }

    #[test]
    fn prop_build_ribbon_slices_preserve_all_non_empty_candidates(
        unary_costs in vec(0_u16..=200_u16, 2..=4),
    ) {
        let frame = with_block_aspect_ratio(&base_frame(), 1.0);
        let centerline = vec![CenterSample {
            pos: Point {
                row: 10.5,
                col: 10.5,
            },
            tangent_row: 0.0,
            tangent_col: 1.0,
        }];
        let ordered = ordered_candidates(
            unary_costs
                .iter()
                .copied()
                .enumerate()
                .map(|(index, unary_cost)| CellCandidate {
                    state: Some(generated_state(index)),
                    unary_cost: u64::from(unary_cost),
                })
                .chain(std::iter::once(CellCandidate {
                    state: None,
                    unary_cost: 1_000,
                }))
                .collect::<Vec<_>>(),
        );
        let candidates = BTreeMap::from([((10_i64, 10_i64), ordered.clone())]);
        let expected = non_empty_candidates(&ordered);
        let slices = build_ribbon_slices(&centerline, &candidates, &frame);

        prop_assert_eq!(slices.len(), 1);
        prop_assert_eq!(slices[0].cells.len(), 1);
        prop_assert_eq!(&slices[0].cells[0].non_empty_candidates, &expected);
    }

    #[test]
    fn prop_solve_ribbon_dp_matches_reference_solver(
        slice_specs in small_slice_sequence_strategy(),
        spatial_weight_q10 in 0_u32..=2_048_u32,
    ) {
        let slices = slice_specs
            .iter()
            .enumerate()
            .map(|(slice_index, spec)| {
                slice_from_generated_spec(spec, slice_index, slice_specs.len())
            })
            .collect::<Vec<_>>();

        prop_assert_eq!(
            solve_ribbon_dp(&slices, spatial_weight_q10),
            reference_solve_ribbon_dp(&slices, spatial_weight_q10),
        );
    }
}

use super::*;
use crate::test_support::proptest::pure_config;
use proptest::collection::vec;
use proptest::prelude::*;
use std::sync::Arc;

type Coord = (i64, i64);
type CompiledSpec = (Coord, u8, u8, u16, u16);

fn candidate_coord() -> impl Strategy<Value = Coord> {
    (8_i64..=14_i64, 8_i64..=14_i64)
}

fn compiled_specs(max_len: usize) -> BoxedStrategy<Vec<CompiledSpec>> {
    vec(
        (
            candidate_coord(),
            0_u8..=16_u8,
            0_u8..=3_u8,
            0_u16..=4095_u16,
            0_u16..=4095_u16,
        ),
        0..=max_len,
    )
    .boxed()
}

fn previous_specs(max_len: usize) -> BoxedStrategy<Vec<(Coord, u8)>> {
    vec((candidate_coord(), 1_u8..=16_u8), 0..=max_len).boxed()
}

fn compact_origins(max_len: usize) -> BoxedStrategy<Vec<(i16, i16)>> {
    vec((8_i16..=18_i16, 8_i16..=18_i16), 1..=max_len).boxed()
}

fn tile_for_pattern(pattern: u8, sample_q12: u16) -> latent_field::MicroTile {
    match pattern % 4 {
        0 => tile_for_column_span(0, latent_field::MICRO_W - 1, sample_q12),
        1 => tile_for_column_span(0, latent_field::MICRO_W / 2, sample_q12),
        2 => tile_for_column_span(
            latent_field::MICRO_W / 2,
            latent_field::MICRO_W - 1,
            sample_q12,
        ),
        _ => tile_for_octant(0x33, sample_q12),
    }
}

fn sample_q12_for_level(raw_level: u8) -> u16 {
    if raw_level == 0 {
        0
    } else {
        quantized_level_to_sample_q12(HighlightLevel::from_raw_clamped(u32::from(raw_level)), 16)
    }
}

fn compiled_map(specs: &[CompiledSpec]) -> BTreeMap<Coord, latent_field::CompiledCell> {
    specs
        .iter()
        .copied()
        .map(
            |(coord, raw_level, pattern, total_mass_q12, recent_mass_q12)| {
                (
                    coord,
                    latent_field::CompiledCell {
                        tile: tile_for_pattern(pattern, sample_q12_for_level(raw_level)),
                        age: AgeMoment {
                            total_mass_q12: u32::from(total_mass_q12),
                            recent_mass_q12: u32::from(recent_mass_q12.min(total_mass_q12)),
                        },
                    },
                )
            },
        )
        .collect()
}

fn previous_cells(specs: &[(Coord, u8)]) -> BTreeMap<Coord, DecodedCellState> {
    specs
        .iter()
        .copied()
        .map(|(coord, level)| (coord, highlight_state(u32::from(level))))
        .collect()
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_candidate_population_keeps_union_keys_sorted_unique_and_top_k_bounded(
        compiled_specs in compiled_specs(20),
        previous_specs in previous_specs(20),
        temporal_stability_weight in 0.0_f64..3.0_f64,
        top_k in 0_usize..=8_usize,
    ) {
        let compiled = compiled_map(&compiled_specs);
        let previous_cells = previous_cells(&previous_specs);
        let candidates =
            build_cell_candidates(&compiled, &previous_cells, 16, temporal_stability_weight, top_k);
        let expected_keys = compiled
            .keys()
            .chain(previous_cells.keys())
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        prop_assert_eq!(candidates.keys().copied().collect::<Vec<_>>(), expected_keys);

        for (coord, per_cell_candidates) in &candidates {
            let previous = previous_cells.get(coord).copied();
            let mut sorted = per_cell_candidates.clone();
            sorted.sort_by(|lhs, rhs| candidate_cmp(*lhs, *rhs, previous));
            let non_empty_states = per_cell_candidates
                .iter()
                .filter_map(|candidate| candidate.state)
                .collect::<Vec<_>>();

            prop_assert!(!per_cell_candidates.is_empty());
            prop_assert_eq!(
                per_cell_candidates
                    .iter()
                    .filter(|candidate| candidate.state.is_none())
                    .count(),
                1
            );
            prop_assert_eq!(per_cell_candidates, &sorted);
            prop_assert_eq!(
                non_empty_states.len(),
                non_empty_states
                    .iter()
                    .copied()
                    .collect::<BTreeSet<_>>()
                    .len()
            );
            prop_assert!(per_cell_candidates.len() <= top_k.max(1));

            if !compiled.contains_key(coord) || top_k <= 1 {
                prop_assert_eq!(per_cell_candidates.len(), 1);
                prop_assert_eq!(per_cell_candidates[0].state, None);
            }
        }
    }

    #[test]
    fn prop_compile_render_frame_reuses_cached_field_for_quiescent_frames(
        row in 8_i16..=18_i16,
        col in 8_i16..=18_i16,
    ) {
        let frame = quiescent_frame(row, col);

        let first = compile_render_frame(&frame, PlannerState::default());
        let second = compile_render_frame(&frame, first.next_state.clone());

        prop_assert!(Arc::ptr_eq(&first.compiled, &second.compiled));
        prop_assert_eq!(first.query_bounds, second.query_bounds);
    }

    #[test]
    fn prop_compile_render_frame_invalidates_cached_field_after_staged_history_changes(
        first_row in 8_i16..=18_i16,
        first_col in 8_i16..=18_i16,
        second_row in 8_i16..=18_i16,
        second_col in 8_i16..=18_i16,
    ) {
        let first = compile_render_frame(
            &single_sample_frame(first_row, first_col),
            PlannerState::default(),
        );
        let second = compile_render_frame(
            &single_sample_frame(second_row, second_col),
            first.next_state.clone(),
        );

        prop_assert!(!Arc::ptr_eq(&first.compiled, &second.compiled));
    }

    #[test]
    fn prop_compile_render_frame_matches_reference_and_stays_on_fast_path_for_compact_histories(
        origins in compact_origins(4),
    ) {
        let mut state = PlannerState::default();

        for origin in origins {
            let frame = single_sample_frame(origin.0, origin.1);
            let reference_state = state.clone();
            let compiled = compile_render_frame(&frame, state);
            let mut reference = compile_render_frame_reference(&frame, reference_state);

            prop_assert!(
                compiled.query_bounds.is_some(),
                "compact staged motion should stay on the bounded planner fast path",
            );
            prop_assert!(matches!(compiled.compiled.as_ref(), CompiledField::Rows(_)));

            if let Some(bounds) = compiled.query_bounds {
                reference.retain(|coord, _| bounds.contains(*coord));
            }

            prop_assert_eq!(compiled.compiled.to_btree_map(), reference);
            state = compiled.next_state;
        }
    }

    #[test]
    fn prop_compile_render_frame_falls_back_to_reference_when_previous_halo_exceeds_budget(
        far_span in LOCAL_QUERY_ENVELOPE_FAST_PATH_MAX_AREA_CELLS as i64
            ..= LOCAL_QUERY_ENVELOPE_FAST_PATH_MAX_AREA_CELLS as i64 + 512_i64,
        left_level in 1_u8..=16_u8,
        right_level in 1_u8..=16_u8,
    ) {
        let frame = base_frame();
        let state = PlannerState {
            previous_cells: Arc::new(BTreeMap::from([
                ((8_i64, 8_i64), highlight_state(u32::from(left_level))),
                ((8_i64, 8_i64 + far_span), highlight_state(u32::from(right_level))),
            ])),
            ..PlannerState::default()
        };

        let compiled = compile_render_frame(&frame, state.clone());
        let reference = compile_render_frame_reference(&frame, state);
        let envelope = compute_local_query_envelope(
            &compiled.next_state.decode_scratch.centerline,
            &compiled.next_state.previous_cells,
            &frame,
            PREVIOUS_CELL_HALO_CELLS,
        )
        .expect("oversized previous-cell halos should still produce a finite envelope");

        prop_assert!(
            query_envelope_area_cells(envelope) > LOCAL_QUERY_ENVELOPE_FAST_PATH_MAX_AREA_CELLS
        );
        prop_assert_eq!(compiled.query_bounds, None);
        prop_assert!(matches!(compiled.compiled.as_ref(), CompiledField::Reference(_)));
        prop_assert_eq!(compiled.compiled.to_btree_map(), reference);
    }
}

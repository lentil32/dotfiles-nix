use super::*;
use crate::test_support::proptest::pure_config;
use proptest::collection::vec;
use proptest::prelude::*;

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

fn search_bounds() -> BoxedStrategy<SliceSearchBounds> {
    (
        7_i64..=15_i64,
        7_i64..=15_i64,
        7_i64..=15_i64,
        7_i64..=15_i64,
    )
        .prop_map(|(row_a, row_b, col_a, col_b)| {
            SliceSearchBounds::new(
                row_a.min(row_b),
                row_a.max(row_b),
                col_a.min(col_b),
                col_a.max(col_b),
            )
        })
        .boxed()
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
        _ => tile_for_octant(0x55, sample_q12),
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

fn candidate_capacities(
    candidates: &BTreeMap<Coord, Vec<CellCandidate>>,
) -> BTreeMap<Coord, usize> {
    candidates
        .iter()
        .map(|(&coord, per_cell)| (coord, per_cell.capacity()))
        .collect()
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_render_plan_matches_reference_compile_on_generated_frame_sequences(
        origins in compact_origins(5),
    ) {
        let frames = frames_from_origins(&origins);
        let viewport = test_viewport();
        let mut optimized_state = PlannerState::default();
        let mut reference_state = PlannerState::default();

        for frame in &frames {
            let optimized = render_frame_to_plan(frame, optimized_state, viewport);
            let reference = render_frame_to_plan_reference(frame, reference_state, viewport);

            prop_assert_eq!(optimized.plan, reference.plan);
            prop_assert_eq!(optimized.signature, reference.signature);
            prop_assert_eq!(&optimized.next_state, &reference.next_state);

            optimized_state = optimized.next_state;
            reference_state = reference.next_state;
        }
    }

    #[test]
    fn prop_bounded_candidate_population_matches_reference_filtered_to_same_rect(
        compiled_specs in compiled_specs(20),
        previous_specs in previous_specs(20),
        bounds in search_bounds(),
        temporal_stability_weight in 0.0_f64..3.0_f64,
        top_k in 0_usize..=8_usize,
    ) {
        let compiled = compiled_map(&compiled_specs);
        let previous_cells = previous_cells(&previous_specs);
        let mut expected =
            build_cell_candidates(&compiled, &previous_cells, 16, temporal_stability_weight, top_k);
        expected.retain(|coord, _| bounds.contains(*coord));

        let compiled = CompiledField::Rows(compiled_rows(&compiled));
        let mut scratch = PlannerDecodeScratch::default();
        populate_cell_candidates_in_bounds_with_scratch(
            &compiled,
            &previous_cells,
            16,
            temporal_stability_weight,
            top_k,
            bounds,
            &mut scratch,
        );

        prop_assert_eq!(scratch.cell_candidates, expected);
    }

    #[test]
    fn prop_repeated_candidate_population_keeps_stable_output_and_per_cell_capacity(
        compiled_specs in compiled_specs(20),
        previous_specs in previous_specs(20),
        temporal_stability_weight in 0.0_f64..3.0_f64,
        top_k in 0_usize..=8_usize,
    ) {
        let compiled = CompiledField::Rows(compiled_rows(&compiled_map(&compiled_specs)));
        let previous_cells = previous_cells(&previous_specs);
        let mut scratch = PlannerDecodeScratch::default();

        populate_cell_candidates_with_scratch(
            &compiled,
            &previous_cells,
            16,
            temporal_stability_weight,
            top_k,
            &mut scratch,
        );
        let first_candidates = scratch.cell_candidates.clone();
        let first_capacities = candidate_capacities(&scratch.cell_candidates);

        populate_cell_candidates_with_scratch(
            &compiled,
            &previous_cells,
            16,
            temporal_stability_weight,
            top_k,
            &mut scratch,
        );
        let second_capacities = candidate_capacities(&scratch.cell_candidates);

        prop_assert!(first_capacities.values().all(|capacity| *capacity > 0));
        prop_assert_eq!(scratch.cell_candidates, first_candidates);
        prop_assert_eq!(second_capacities, first_capacities);
    }

    #[test]
    fn prop_planner_idle_steps_age_history_without_new_motion_samples(
        row in 8_i16..=18_i16,
        col in 8_i16..=18_i16,
        extra_idle_steps in 0_u16..=4_u16,
    ) {
        let viewport = test_viewport();
        let first = render_frame_to_plan(&single_sample_frame(row, col), PlannerState::default(), viewport);
        let mut draining = quiescent_frame(row, col);
        draining.planner_idle_steps = u32::try_from(latent_field::max_comet_support_steps(
            draining.tail_duration_ms,
            draining.simulation_hz,
        ))
        .unwrap_or(u32::MAX)
        .saturating_add(u32::from(extra_idle_steps));

        let drained = render_frame_to_plan(&draining, first.next_state, viewport);

        prop_assert!(drained.plan.cell_ops.is_empty());
        prop_assert!(drained.next_state.previous_cells.is_empty());
    }
}

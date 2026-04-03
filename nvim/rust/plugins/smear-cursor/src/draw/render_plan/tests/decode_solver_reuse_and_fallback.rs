use super::*;
use crate::test_support::proptest::positive_aspect_ratio;
use crate::test_support::proptest::pure_config;
use proptest::collection::btree_map;
use proptest::collection::btree_set;
use proptest::prelude::*;
use std::collections::VecDeque;

type Coord = (i64, i64);

fn decoded_glyph_strategy() -> BoxedStrategy<DecodedGlyph> {
    prop_oneof![
        Just(DecodedGlyph::Block),
        (1_u8..=15_u8).prop_map(DecodedGlyph::Matrix),
        (1_u8..=255_u8).prop_map(DecodedGlyph::Octant),
    ]
    .boxed()
}

fn decoded_state_strategy() -> BoxedStrategy<DecodedCellState> {
    (decoded_glyph_strategy(), 1_u32..=16_u32)
        .prop_map(|(glyph, level)| DecodedCellState {
            glyph,
            level: HighlightLevel::from_raw_clamped(level),
        })
        .boxed()
}

fn same_level_distinct_state_pair() -> BoxedStrategy<(DecodedCellState, DecodedCellState)> {
    (
        decoded_glyph_strategy(),
        decoded_glyph_strategy(),
        1_u32..=16_u32,
    )
        .prop_filter("states must differ", |(lhs, rhs, _)| lhs != rhs)
        .prop_map(|(lhs, rhs, level)| {
            let level = HighlightLevel::from_raw_clamped(level);
            (
                DecodedCellState { glyph: lhs, level },
                DecodedCellState { glyph: rhs, level },
            )
        })
        .boxed()
}

fn presence_candidates(
    state: DecodedCellState,
    filled_cost: u16,
    empty_cost: u16,
) -> Vec<CellCandidate> {
    ordered_candidates(vec![
        CellCandidate {
            state: Some(state),
            unary_cost: u64::from(filled_cost),
        },
        CellCandidate {
            state: None,
            unary_cost: u64::from(empty_cost),
        },
    ])
}

fn horizontal_centerline(points: impl IntoIterator<Item = Point>) -> Vec<CenterSample> {
    points
        .into_iter()
        .map(|pos| CenterSample {
            pos,
            tangent_row: 0.0,
            tangent_col: 1.0,
        })
        .collect()
}

fn reference_active_support_is_disconnected(coords: &BTreeSet<Coord>) -> bool {
    if coords.len() > FALLBACK_COMPONENT_THRESHOLD {
        return true;
    }
    if coords.len() < 3 {
        return false;
    }

    let mut unvisited = coords.clone();
    let mut component_count = 0_usize;
    while let Some(seed) = unvisited.iter().next().copied() {
        component_count += 1;
        let mut queue = VecDeque::from([seed]);
        let _ = unvisited.remove(&seed);

        while let Some((row, col)) = queue.pop_front() {
            for neighbor_row in (row - 1)..=(row + 1) {
                for neighbor_col in (col - 1)..=(col + 1) {
                    if neighbor_row == row && neighbor_col == col {
                        continue;
                    }
                    let neighbor = (neighbor_row, neighbor_col);
                    if unvisited.remove(&neighbor) {
                        queue.push_back(neighbor);
                    }
                }
            }
        }
    }

    component_count > 1
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_pairwise_fallback_scratch_reuse_keeps_output_and_capacity_stable(
        candidates in btree_map(
            (8_i64..=18_i64, 8_i64..=18_i64),
            (decoded_state_strategy(), 0_u16..=512_u16, 0_u16..=512_u16),
            1..=24,
        )
        .prop_map(|entries| {
            entries
                .into_iter()
                .map(|(coord, (state, filled_cost, empty_cost))| {
                    (coord, presence_candidates(state, filled_cost, empty_cost))
                })
                .collect::<BTreeMap<_, _>>()
        }),
        spatial_weight_q10 in 0_u32..=2048_u32,
    ) {
        let mut scratch = SolverScratch::default();

        let first =
            solve_pairwise_fallback_with_scratch(&candidates, spatial_weight_q10, &mut scratch);
        let first_capacities = (
            scratch.fallback_coords.capacity(),
            scratch.fallback_coord_index.capacity(),
            scratch.fallback_assignment.capacity(),
        );

        let second =
            solve_pairwise_fallback_with_scratch(&candidates, spatial_weight_q10, &mut scratch);

        prop_assert!(first_capacities.0 > 0);
        prop_assert!(first_capacities.1 > 0);
        prop_assert!(first_capacities.2 > 0);
        prop_assert_eq!(second, first);
        prop_assert_eq!(
            (
                scratch.fallback_coords.capacity(),
                scratch.fallback_coord_index.capacity(),
                scratch.fallback_assignment.capacity(),
            ),
            first_capacities,
        );
    }

    #[test]
    fn prop_decode_trace_scratch_reuse_keeps_output_and_row_index_capacity_stable(
        generated in (
            btree_map(
                (8_i64..=18_i64, 8_i64..=18_i64),
                (
                    0_u8..=3_u8,
                    0_u16..=4095_u16,
                    0_u16..=4095_u16,
                    decoded_state_strategy(),
                    0_u16..=512_u16,
                    0_u16..=512_u16,
                ),
                1..=16,
            ),
            positive_aspect_ratio(),
        )
            .prop_map(|(entries, block_aspect_ratio)| {
                let compiled = entries
                    .iter()
                    .map(
                        |(&(row, col), (pattern, total_mass_q12, recent_mass_q12, ..))| {
                            let tile = match pattern % 4 {
                                0 => {
                                    tile_for_column_span(0, latent_field::MICRO_W - 1, 0x0FFF)
                                }
                                1 => tile_for_column_span(0, latent_field::MICRO_W / 2, 0x09FF),
                                2 => tile_for_column_span(
                                    latent_field::MICRO_W / 2,
                                    latent_field::MICRO_W - 1,
                                    0x09FF,
                                ),
                                _ => tile_for_octant(0x55, 0x09FF),
                            };
                            (
                                (row, col),
                                latent_field::CompiledCell {
                                    tile,
                                    age: AgeMoment {
                                        total_mass_q12: u32::from(*total_mass_q12),
                                        recent_mass_q12: u32::from(
                                            (*recent_mass_q12).min(*total_mass_q12),
                                        ),
                                    },
                                },
                            )
                        },
                    )
                    .collect::<BTreeMap<_, _>>();
                let candidates = entries
                    .into_iter()
                    .map(
                        |(coord, (_, _, _, state, filled_cost, empty_cost))| {
                            (coord, presence_candidates(state, filled_cost, empty_cost))
                        },
                    )
                    .collect::<BTreeMap<_, _>>();
                let centerline = horizontal_centerline(compiled.keys().copied().map(|(row, col)| {
                    Point {
                        row: row as f64 + 0.5,
                        col: col as f64 + 0.5,
                    }
                }));

                (compiled, candidates, centerline, block_aspect_ratio)
            }),
    ) {
        let (compiled, candidates, centerline, block_aspect_ratio) = generated;
        let compiled = compiled_field(&compiled);
        let frame = with_block_aspect_ratio(&base_frame(), block_aspect_ratio);
        let mut scratch = SolverScratch::default();

        let first = decode_compiled_field_trace_with_compiled_and_scratch(
            &compiled,
            &candidates,
            &centerline,
            &frame,
            &mut scratch,
        );
        let first_capacities = (
            scratch.compiled_row_index.row_capacity(),
            scratch.compiled_row_index.entry_capacity(),
            scratch.candidate_row_index.row_capacity(),
            scratch.candidate_row_index.entry_capacity(),
        );

        let second = decode_compiled_field_trace_with_compiled_and_scratch(
            &compiled,
            &candidates,
            &centerline,
            &frame,
            &mut scratch,
        );

        prop_assert!(first_capacities.0 > 0);
        prop_assert!(first_capacities.1 > 0);
        prop_assert!(first_capacities.2 > 0);
        prop_assert!(first_capacities.3 > 0);
        prop_assert_eq!(second.cells, first.cells);
        prop_assert_eq!(second.path, first.path);
        prop_assert_eq!(
            (
                scratch.compiled_row_index.row_capacity(),
                scratch.compiled_row_index.entry_capacity(),
                scratch.candidate_row_index.row_capacity(),
                scratch.candidate_row_index.entry_capacity(),
            ),
            first_capacities,
        );
    }

    #[test]
    fn prop_merge_ribbon_assignments_breaks_equal_vote_ties_by_state_order(
        (lhs_state, rhs_state) in same_level_distinct_state_pair(),
        unary_cost in 0_u16..=512_u16,
        empty_cost in 0_u16..=512_u16,
    ) {
        let coord = (10_i64, 10_i64);
        let candidates = ordered_candidates(vec![
            CellCandidate {
                state: Some(lhs_state),
                unary_cost: u64::from(unary_cost),
            },
            CellCandidate {
                state: Some(rhs_state),
                unary_cost: u64::from(unary_cost),
            },
        ]);
        let slice = RibbonSlice {
            cells: vec![slice_cell_with_candidates(coord, 0.0, 0.5, u64::from(empty_cost), candidates.clone())],
            tail_u: 0.5,
            target_width_cells: 1.0,
            tip_width_cap_cells: 1.0,
            transverse_width_penalty: 0.0,
        };
        let run = RunSpan::try_new(0, 0).expect("single-cell slice should build a run");
        let choose_first = SliceState::with_run(run, [0; RIBBON_MAX_RUN_LENGTH], 0);
        let mut second_offsets = [0; RIBBON_MAX_RUN_LENGTH];
        second_offsets[0] = 1;
        let choose_second = SliceState::with_run(run, second_offsets, 0);
        let mut scratch = SolverScratch::default();
        let mut forward = BTreeMap::new();
        let mut reversed = BTreeMap::new();

        merge_ribbon_assignments(
            &mut forward,
            &[slice.clone(), slice.clone()],
            &[choose_second, choose_first],
            &mut scratch,
        );
        merge_ribbon_assignments(
            &mut reversed,
            &[slice.clone(), slice],
            &[choose_first, choose_second],
            &mut scratch,
        );

        let expected = candidates
            .iter()
            .filter_map(|candidate| candidate.state)
            .min_by_key(|state| state_sort_key(Some(*state)))
            .expect("fixture always provides non-empty candidates");

        prop_assert_eq!(&forward, &reversed);
        prop_assert_eq!(forward.get(&coord).copied(), Some(expected));
    }

    #[test]
    fn prop_active_support_disconnected_matches_reference_component_model(
        coords in btree_set((0_i64..=12_i64, 0_i64..=12_i64), 0..=110),
    ) {
        let active_cells = coords
            .iter()
            .copied()
            .map(|coord| (coord, highlight_state(8)))
            .collect::<BTreeMap<_, _>>();

        prop_assert_eq!(
            active_support_is_disconnected(&active_cells),
            reference_active_support_is_disconnected(&coords),
        );
    }

    #[test]
    fn prop_disconnected_support_chooses_pairwise_fallback_path(
        row in 8_i64..=18_i64,
        left_col in 8_i64..=18_i64,
        left_width in 1_u8..=3_u8,
        gap_width in 3_u8..=6_u8,
        right_width in 1_u8..=3_u8,
        left_level in 1_u32..=8_u32,
        right_level in 9_u32..=16_u32,
    ) {
        prop_assume!(usize::from(left_width) + usize::from(right_width) >= 3);

        let left_width = i64::from(left_width);
        let right_width = i64::from(right_width);
        let right_col = left_col + left_width + i64::from(gap_width);
        let left_state = highlight_state(left_level);
        let right_state = highlight_state(right_level);
        let candidates = ((left_col..(left_col + left_width))
            .map(|col| ((row, col), presence_candidates(left_state, 0, 40_000)))
            .chain(
                (right_col..(right_col + right_width))
                    .map(|col| ((row, col), presence_candidates(right_state, 0, 40_000))),
            ))
        .collect::<BTreeMap<_, _>>();
        let baseline = decode_locally(&candidates);
        let baseline_coords = baseline.keys().copied().collect::<BTreeSet<_>>();
        let centerline = horizontal_centerline([
            Point {
                row: row as f64 + 0.5,
                col: left_col as f64 + left_width as f64 / 2.0,
            },
            Point {
                row: row as f64 + 0.5,
                col: right_col as f64 + right_width as f64 / 2.0,
            },
        ]);
        let frame = with_block_aspect_ratio(&base_frame(), 1.0);
        let slices = build_ribbon_slices(&centerline, &candidates, &frame);

        prop_assert!(reference_active_support_is_disconnected(&baseline_coords));
        prop_assert!(active_support_is_disconnected(&baseline));
        prop_assert_eq!(
            select_decode_path(&baseline, &slices, sanitize_spatial_weight_q10(&frame)),
            DecodePathTrace::PairwiseFallbackDisconnected,
        );
        prop_assert_eq!(
            decode_compiled_field(&candidates, &centerline, &frame),
            solve_pairwise_fallback(&candidates, sanitize_spatial_weight_q10(&frame)),
        );
    }
}

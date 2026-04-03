use super::*;
use crate::test_support::proptest::pure_config;
use proptest::prelude::*;

fn diagonal_centerline(origin_row: i64, origin_col: i64, len: usize) -> Vec<CenterSample> {
    let tangent = 1.0 / 2.0_f64.sqrt();
    (0..len)
        .map(|offset| {
            let offset = i64::try_from(offset).expect("centerline length fits in i64");
            CenterSample {
                pos: Point {
                    row: (origin_row + offset) as f64 + 0.5,
                    col: (origin_col + offset) as f64 + 0.5,
                },
                tangent_row: tangent,
                tangent_col: tangent,
            }
        })
        .collect()
}

fn sparse_gap_fixture(
    origin_row: i64,
    origin_col: i64,
    tail_level: u32,
    head_level: u32,
) -> BTreeMap<(i64, i64), Vec<CellCandidate>> {
    BTreeMap::from([
        (
            (origin_row, origin_col),
            ordered_candidates(vec![
                CellCandidate {
                    state: Some(highlight_state(tail_level)),
                    unary_cost: 0,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 40_000,
                },
            ]),
        ),
        (
            (origin_row + 1, origin_col + 1),
            ordered_candidates(vec![
                CellCandidate {
                    state: Some(highlight_state(tail_level)),
                    unary_cost: 0,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 40_000,
                },
            ]),
        ),
        (
            (origin_row + 2, origin_col + 2),
            ordered_candidates(vec![CellCandidate {
                state: None,
                unary_cost: 0,
            }]),
        ),
        (
            (origin_row + 3, origin_col + 3),
            ordered_candidates(vec![
                CellCandidate {
                    state: Some(highlight_state(head_level)),
                    unary_cost: 0,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 40_000,
                },
            ]),
        ),
        (
            (origin_row + 4, origin_col + 4),
            ordered_candidates(vec![
                CellCandidate {
                    state: Some(highlight_state(head_level)),
                    unary_cost: 0,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 40_000,
                },
            ]),
        ),
    ])
}

fn brighter_head_fixture(
    origin_row: i64,
    origin_col: i64,
    tail_level: u32,
    dim_level: u32,
    bright_level: u32,
    bright_penalty: u64,
) -> BTreeMap<(i64, i64), Vec<CellCandidate>> {
    BTreeMap::from([
        (
            (origin_row, origin_col),
            ordered_candidates(vec![
                CellCandidate {
                    state: Some(highlight_state(tail_level)),
                    unary_cost: 0,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 40_000,
                },
            ]),
        ),
        (
            (origin_row + 1, origin_col + 1),
            ordered_candidates(vec![
                CellCandidate {
                    state: Some(highlight_state(tail_level)),
                    unary_cost: 0,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 40_000,
                },
            ]),
        ),
        (
            (origin_row + 2, origin_col + 2),
            ordered_candidates(vec![
                CellCandidate {
                    state: Some(highlight_state(dim_level)),
                    unary_cost: 0,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 40_000,
                },
            ]),
        ),
        (
            (origin_row + 3, origin_col + 3),
            ordered_candidates(vec![
                CellCandidate {
                    state: Some(highlight_state(dim_level)),
                    unary_cost: 0,
                },
                CellCandidate {
                    state: Some(highlight_state(bright_level)),
                    unary_cost: bright_penalty,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 40_000,
                },
            ]),
        ),
        (
            (origin_row + 4, origin_col + 4),
            ordered_candidates(vec![
                CellCandidate {
                    state: Some(highlight_state(dim_level)),
                    unary_cost: 0,
                },
                CellCandidate {
                    state: Some(highlight_state(bright_level)),
                    unary_cost: bright_penalty,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 40_000,
                },
            ]),
        ),
    ])
}

fn oversized_support_fixture(
    origin_row: i64,
    origin_col: i64,
) -> BTreeMap<(i64, i64), Vec<CellCandidate>> {
    let mut candidates = BTreeMap::<(i64, i64), Vec<CellCandidate>>::new();
    for row in origin_row..=(origin_row + 6) {
        candidates.insert(
            (row, origin_col),
            ordered_candidates(vec![
                CellCandidate {
                    state: Some(highlight_state(2)),
                    unary_cost: 0,
                },
                CellCandidate {
                    state: Some(highlight_state(6)),
                    unary_cost: 100,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 40_000,
                },
            ]),
        );
        candidates.insert(
            (row, origin_col + 1),
            ordered_candidates(vec![
                CellCandidate {
                    state: Some(highlight_state(6)),
                    unary_cost: 0,
                },
                CellCandidate {
                    state: Some(highlight_state(2)),
                    unary_cost: 100,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 40_000,
                },
            ]),
        );
        candidates.insert(
            (row, origin_col + 2),
            ordered_candidates(vec![
                CellCandidate {
                    state: Some(highlight_state(2)),
                    unary_cost: 0,
                },
                CellCandidate {
                    state: Some(highlight_state(6)),
                    unary_cost: 100,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 40_000,
                },
            ]),
        );
    }
    candidates
}

fn increasing_tail_head_levels() -> BoxedStrategy<(u32, u32)> {
    (1_u32..=15_u32)
        .prop_flat_map(|tail_level| (Just(tail_level), (tail_level + 1)..=16_u32))
        .boxed()
}

fn salience_ladder() -> BoxedStrategy<(u32, u32, u32)> {
    (1_u32..=10_u32)
        .prop_flat_map(|tail_level| {
            (Just(tail_level), (tail_level + 1)..=14_u32).prop_flat_map(
                |(tail_level, dim_level)| {
                    (Just(tail_level), Just(dim_level), (dim_level + 1)..=16_u32)
                },
            )
        })
        .boxed()
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_sparse_undecodable_gap_keeps_ribbon_path_and_destination_salience(
        origin_row in 8_i64..=16_i64,
        origin_col in 8_i64..=16_i64,
        (tail_level, head_level) in increasing_tail_head_levels(),
    ) {
        let frame = with_block_aspect_ratio(&base_frame(), 1.0);
        let centerline = diagonal_centerline(origin_row, origin_col, 5);
        let candidates = sparse_gap_fixture(origin_row, origin_col, tail_level, head_level);
        let baseline = decode_locally(&candidates);
        let decoded = decode_compiled_field_trace(&candidates, &centerline, &frame);
        let tail_coord = (origin_row, origin_col);
        let gap_coord = (origin_row + 2, origin_col + 2);
        let head_coord = (origin_row + 4, origin_col + 4);
        let tail = decoded
            .cells
            .get(&tail_coord)
            .expect("tail cell should stay decoded");
        let head = decoded
            .cells
            .get(&head_coord)
            .expect("destination catch should stay decoded");

        prop_assert!(active_support_is_disconnected(&baseline));
        prop_assert_eq!(decoded.path, DecodePathTrace::RibbonDp);
        prop_assert!(!decoded.cells.contains_key(&gap_coord));
        prop_assert!(head.level.value() > tail.level.value());
    }

    #[test]
    fn prop_destination_catch_prefers_brighter_head_state_over_slightly_cheaper_dim_state(
        origin_row in 8_i64..=16_i64,
        origin_col in 8_i64..=16_i64,
        (tail_level, dim_level, bright_level) in salience_ladder(),
        bright_penalty in 1_u64..=48_u64,
    ) {
        let frame = with_block_aspect_ratio(&base_frame(), 1.0);
        let centerline = diagonal_centerline(origin_row, origin_col, 5);
        let candidates = brighter_head_fixture(
            origin_row,
            origin_col,
            tail_level,
            dim_level,
            bright_level,
            bright_penalty,
        );
        let decoded = decode_compiled_field_trace(&candidates, &centerline, &frame);
        let tail = decoded
            .cells
            .get(&(origin_row, origin_col))
            .expect("tail cell should stay decoded");
        let head = decoded
            .cells
            .get(&(origin_row + 4, origin_col + 4))
            .expect("destination catch should stay decoded");

        prop_assert_eq!(decoded.path, DecodePathTrace::RibbonDp);
        prop_assert_eq!(head.level.value(), bright_level);
        prop_assert!(head.level.value() > tail.level.value());
    }

    #[test]
    fn prop_oversized_ribbon_support_uses_pairwise_fallback_instead_of_local_baseline(
        origin_row in 7_i64..=15_i64,
        origin_col in 9_i64..=15_i64,
    ) {
        let frame = with_trail_thickness(&with_block_aspect_ratio(&base_frame(), 1.0), 4.0);
        let centerline = vec![
            CenterSample {
                pos: Point {
                    row: origin_row as f64 + 3.5,
                    col: origin_col as f64 + 1.0,
                },
                tangent_row: 0.0,
                tangent_col: 1.0,
            },
            CenterSample {
                pos: Point {
                    row: origin_row as f64 + 3.5,
                    col: origin_col as f64 + 2.0,
                },
                tangent_row: 0.0,
                tangent_col: 1.0,
            },
        ];
        let candidates = oversized_support_fixture(origin_row, origin_col);
        let slices = build_ribbon_slices(&centerline, &candidates, &frame);
        let baseline = decode_locally(&candidates);
        let fallback = solve_pairwise_fallback(&candidates, sanitize_spatial_weight_q10(&frame));

        prop_assert!(ribbon_support_is_oversized(&slices));
        prop_assert_eq!(
            select_decode_path(&baseline, &slices, sanitize_spatial_weight_q10(&frame)),
            DecodePathTrace::PairwiseFallbackOversized,
        );
        prop_assert_eq!(decode_compiled_field(&candidates, &centerline, &frame), fallback);
    }
}

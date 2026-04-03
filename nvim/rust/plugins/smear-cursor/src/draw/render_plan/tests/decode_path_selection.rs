use super::*;
use crate::test_support::proptest::pure_config;
use proptest::prelude::*;

type Coord = (i64, i64);

fn diagonal_centerline(origin: Coord) -> Vec<CenterSample> {
    let tangent = 1.0 / 2.0_f64.sqrt();
    (0_i64..3_i64)
        .map(|offset| CenterSample {
            pos: Point {
                row: (origin.0 + offset) as f64 + 1.0,
                col: (origin.1 + offset) as f64 + 1.5,
            },
            tangent_row: tangent,
            tangent_col: tangent,
        })
        .collect()
}

fn make_presence_candidates(
    state: DecodedCellState,
    non_empty_cost: u64,
    empty_cost: u64,
) -> Vec<CellCandidate> {
    ordered_candidates(vec![
        CellCandidate {
            state: Some(state),
            unary_cost: non_empty_cost,
        },
        CellCandidate {
            state: None,
            unary_cost: empty_cost,
        },
    ])
}

fn elbow_fixture(origin: Coord, state: DecodedCellState) -> BTreeMap<Coord, Vec<CellCandidate>> {
    BTreeMap::from([
        ((origin.0, origin.1), make_presence_candidates(state, 20, 0)),
        (
            (origin.0 + 1, origin.1),
            make_presence_candidates(state, 0, 100),
        ),
        (
            (origin.0 + 1, origin.1 + 1),
            make_presence_candidates(state, 20, 0),
        ),
        (
            (origin.0 + 2, origin.1 + 1),
            make_presence_candidates(state, 0, 100),
        ),
        (
            (origin.0 + 2, origin.1 + 2),
            make_presence_candidates(state, 0, 100),
        ),
        (
            (origin.0 + 3, origin.1 + 2),
            make_presence_candidates(state, 20, 0),
        ),
    ])
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_decode_without_centerline_preserves_connected_local_baseline(
        row in 0_i64..=12_i64,
        start_col in 0_i64..=12_i64,
        width in 1_u8..=6_u8,
        level in 1_u32..=16_u32,
        filled_cost in 0_u16..=128_u16,
        empty_margin in 1_u16..=128_u16,
    ) {
        let candidates = (0..i64::from(width))
            .map(|offset| {
                (
                    (row, start_col + offset),
                    make_presence_candidates(
                        highlight_state(level),
                        u64::from(filled_cost),
                        u64::from(filled_cost.saturating_add(empty_margin)),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let frame = base_frame();

        prop_assert_eq!(decode_compiled_field(&candidates, &[], &frame), decode_locally(&candidates));
    }

    #[test]
    fn prop_ribbon_decode_recovers_diagonal_chain_from_local_elbow(
        origin_row in 8_i64..=16_i64,
        origin_col in 8_i64..=16_i64,
        level in 1_u32..=16_u32,
    ) {
        let origin = (origin_row, origin_col);
        let candidates = elbow_fixture(origin, highlight_state(level));
        let frame = with_block_aspect_ratio(&base_frame(), 1.0);
        let baseline = decode_locally(&candidates);
        let decoded = decode_compiled_field(&candidates, &diagonal_centerline(origin), &frame);
        let baseline_cells = baseline.keys().copied().collect::<BTreeSet<_>>();
        let decoded_cells = decoded.keys().copied().collect::<BTreeSet<_>>();
        let expected_baseline = BTreeSet::from([
            (origin.0 + 1, origin.1),
            (origin.0 + 2, origin.1 + 1),
            (origin.0 + 2, origin.1 + 2),
        ]);
        let expected_diagonal = BTreeSet::from([
            (origin.0 + 1, origin.1),
            (origin.0 + 2, origin.1 + 1),
            (origin.0 + 3, origin.1 + 2),
        ]);

        prop_assert_eq!(baseline_cells, expected_baseline);
        prop_assert!(decoded_cells.is_superset(&expected_diagonal), "decoded={decoded_cells:?}");
    }
}

#[test]
fn ribbon_decode_uses_pairwise_fallback_when_dp_solver_fails() {
    let state = highlight_state(8);
    let candidates = elbow_fixture((9, 10), state);
    let frame = with_block_aspect_ratio(&base_frame(), 1.0);
    let baseline = decode_locally(&candidates);
    let centerline = diagonal_centerline((9, 10));
    let slices = build_ribbon_slices(&centerline, &candidates, &frame);
    assert_eq!(
        select_decode_path(&baseline, &slices, sanitize_spatial_weight_q10(&frame)),
        DecodePathTrace::RibbonDp
    );

    let compiled = CompiledField::default();
    let mut scratch = SolverScratch::default();
    let decoded = decode_compiled_field_with_solver(
        &compiled,
        &candidates,
        &centerline,
        &frame,
        &mut scratch,
        |_, _| None,
    );

    assert_eq!(decoded.path, DecodePathTrace::RibbonDpSolveFailed);
    assert_eq!(
        decoded.cells,
        solve_pairwise_fallback(&candidates, sanitize_spatial_weight_q10(&frame))
    );
}

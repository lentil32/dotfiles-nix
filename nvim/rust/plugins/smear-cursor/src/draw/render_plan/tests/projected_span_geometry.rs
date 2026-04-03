use super::*;
use crate::test_support::proptest::approx_eq_f64;
use crate::test_support::proptest::pure_config;
use proptest::prelude::*;

fn duplicate_band_slice(
    duplicate_count: usize,
    unique_band_count: usize,
    vertical: bool,
) -> RibbonSlice {
    let duplicate_cells = (0..duplicate_count).map(|offset| {
        let coord = if vertical {
            (10 + offset as i64, 10)
        } else {
            (10, 10 + offset as i64)
        };
        slice_cell(coord, 0.0, 0.5, 100, 12, 0)
    });
    let distinct_band_cells = (1..unique_band_count).map(|band| {
        let coord = if vertical {
            (10, 20 + band as i64)
        } else {
            (20 + band as i64, 10)
        };
        slice_cell(coord, band as f64, 0.5, 100, 12, 0)
    });

    RibbonSlice {
        cells: duplicate_cells.chain(distinct_band_cells).collect(),
        tail_u: 0.5,
        target_width_cells: 1.0,
        tip_width_cap_cells: COMET_MIN_RESOLVABLE_WIDTH,
        transverse_width_penalty: 0.0,
    }
}

fn run_state(start: usize, end: usize) -> SliceState {
    SliceState::with_run(
        RunSpan::try_new(start, end).expect("generated duplicate-band runs are valid"),
        [0; RIBBON_MAX_RUN_LENGTH],
        0,
    )
}

fn ordered_q16_bounds() -> BoxedStrategy<(i32, i32)> {
    let limit = 4 * Q16_SCALE;
    (-limit..=limit, -limit..=limit)
        .prop_map(|(first, second)| (first.min(second), first.max(second)))
        .boxed()
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_projected_run_width_ignores_along_axis_duplicates_and_is_orientation_invariant(
        duplicate_count in 1_usize..=2_usize,
        unique_band_count in 1_usize..=3_usize,
    ) {
        let horizontal = duplicate_band_slice(duplicate_count, unique_band_count, false);
        let vertical = duplicate_band_slice(duplicate_count, unique_band_count, true);
        let duplicate_run = run_state(0, duplicate_count - 1);
        let full_run = run_state(0, horizontal.cells.len() - 1);
        let expected_width = unique_band_count as f64;

        prop_assert!(approx_eq_f64(
            run_width_cells(&horizontal, duplicate_run),
            1.0,
            1.0e-6,
        ));
        prop_assert!(approx_eq_f64(
            run_width_cells(&vertical, duplicate_run),
            1.0,
            1.0e-6,
        ));
        prop_assert!(approx_eq_f64(
            run_width_cells(&horizontal, full_run),
            expected_width,
            1.0e-6,
        ));
        prop_assert!(approx_eq_f64(
            run_width_cells(&vertical, full_run),
            expected_width,
            1.0e-6,
        ));
        prop_assert!(approx_eq_f64(
            run_width_cells(&horizontal, duplicate_run),
            run_width_cells(&vertical, duplicate_run),
            1.0e-6,
        ));
        prop_assert!(approx_eq_f64(
            run_width_cells(&horizontal, full_run),
            run_width_cells(&vertical, full_run),
            1.0e-6,
        ));
    }

    #[test]
    fn prop_projected_span_try_new_is_equivalent_to_ordered_bounds(
        min_q16 in -(4 * Q16_SCALE)..=(4 * Q16_SCALE),
        max_q16 in -(4 * Q16_SCALE)..=(4 * Q16_SCALE),
    ) {
        let expected = (min_q16 <= max_q16).then_some(ProjectedSpanQ16 { min_q16, max_q16 });

        prop_assert_eq!(ProjectedSpanQ16::try_new(min_q16, max_q16), expected);
    }

    #[test]
    fn prop_projected_span_cover_is_symmetric_and_covers_both_inputs(
        lhs_bounds in ordered_q16_bounds(),
        rhs_bounds in ordered_q16_bounds(),
    ) {
        let lhs = ProjectedSpanQ16::try_new(lhs_bounds.0, lhs_bounds.1)
            .expect("ordered q16 bounds should always produce a span");
        let rhs = ProjectedSpanQ16::try_new(rhs_bounds.0, rhs_bounds.1)
            .expect("ordered q16 bounds should always produce a span");
        let covered = lhs.cover(rhs);

        prop_assert_eq!(covered, rhs.cover(lhs));
        prop_assert_eq!(covered.min_q16, lhs.min_q16.min(rhs.min_q16));
        prop_assert_eq!(covered.max_q16, lhs.max_q16.max(rhs.max_q16));
        prop_assert!(covered.min_q16 <= lhs.min_q16);
        prop_assert!(covered.min_q16 <= rhs.min_q16);
        prop_assert!(covered.max_q16 >= lhs.max_q16);
        prop_assert!(covered.max_q16 >= rhs.max_q16);
        prop_assert!(covered.width_cells() + 1.0e-6 >= lhs.width_cells());
        prop_assert!(covered.width_cells() + 1.0e-6 >= rhs.width_cells());
    }
}

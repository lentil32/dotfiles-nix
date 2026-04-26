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

}

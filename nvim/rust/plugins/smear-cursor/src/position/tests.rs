use super::RenderPoint;
use super::ScreenCell;
use super::current_visual_cursor_anchor;
use crate::animation::scaled_corners_for_trail;
use crate::test_support::proptest::DEFAULT_FLOAT_EPSILON;
use crate::test_support::proptest::approx_eq_point;
use crate::test_support::proptest::cursor_rectangle;
use crate::test_support::proptest::positive_scale;
use crate::test_support::proptest::pure_config;
use proptest::prelude::*;

fn expected_screen_cell_from_point(point: RenderPoint) -> Option<ScreenCell> {
    if !point.row.is_finite() || !point.col.is_finite() {
        return None;
    }

    let rounded_row = point.row.round();
    let rounded_col = point.col.round();
    if rounded_row < 1.0
        || rounded_col < 1.0
        || rounded_row > i64::MAX as f64
        || rounded_col > i64::MAX as f64
    {
        return None;
    }

    ScreenCell::new(rounded_row as i64, rounded_col as i64)
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_screen_cell_validates_one_indexed_cells(
        row in -8_i64..8_i64,
        col in -8_i64..8_i64,
    ) {
        let expected = (row >= 1 && col >= 1).then(|| {
            ScreenCell::new(row, col).expect("one-based rows and cols should construct")
        });

        prop_assert_eq!(ScreenCell::new(row, col), expected);
    }

    #[test]
    fn prop_screen_cell_from_rounded_point_matches_rounding_and_validity_rules(
        row in prop_oneof![
            -4096.0_f64..4096.0_f64,
            Just(f64::NAN),
            Just(f64::INFINITY),
            Just(f64::NEG_INFINITY),
        ],
        col in prop_oneof![
            -4096.0_f64..4096.0_f64,
            Just(f64::NAN),
            Just(f64::INFINITY),
            Just(f64::NEG_INFINITY),
        ],
    ) {
        let point = RenderPoint { row, col };
        prop_assert_eq!(
            ScreenCell::from_rounded_point(point),
            expected_screen_cell_from_point(point)
        );
    }

    #[test]
    fn prop_visual_cursor_anchor_stays_on_target_under_scaled_head_geometry(
        fixture in cursor_rectangle(),
        row_scale in positive_scale(),
        col_scale in positive_scale(),
    ) {
        let scaled_corners = scaled_corners_for_trail(&fixture.corners, row_scale, col_scale);
        let anchor = current_visual_cursor_anchor(
            &scaled_corners,
            &fixture.corners,
            fixture.position,
        );

        prop_assert!(
            approx_eq_point(anchor, fixture.position, DEFAULT_FLOAT_EPSILON),
            "anchor={anchor:?} target={:?} row_scale={row_scale} col_scale={col_scale}",
            fixture.position
        );
        prop_assert_eq!(
            ScreenCell::from_rounded_point(current_visual_cursor_anchor(
                &scaled_corners,
                &fixture.corners,
                fixture.position,
            )),
            ScreenCell::new(
                fixture.position.row.round() as i64,
                fixture.position.col.round() as i64,
            )
        );
    }
}

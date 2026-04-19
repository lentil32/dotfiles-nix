use super::BufferLine;
use super::CursorObservation;
use super::ObservedCell;
use super::RenderPoint;
use super::ScreenCell;
use super::SurfaceId;
use super::ViewportBounds;
use super::WindowSurfaceSnapshot;
use super::current_visual_cursor_anchor;
use crate::animation::scaled_corners_for_trail;
use crate::test_support::proptest::DEFAULT_FLOAT_EPSILON;
use crate::test_support::proptest::approx_eq_point;
use crate::test_support::proptest::cursor_rectangle;
use crate::test_support::proptest::positive_scale;
use crate::test_support::proptest::pure_config;
use pretty_assertions::assert_eq;
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

fn expected_host_screen_cell_from_point(point: RenderPoint) -> Option<ScreenCell> {
    if !point.row.is_finite()
        || !point.col.is_finite()
        || point.row.fract() != 0.0
        || point.col.fract() != 0.0
        || point.row < 1.0
        || point.col < 1.0
        || point.row > i64::MAX as f64
        || point.col > i64::MAX as f64
    {
        return None;
    }

    ScreenCell::new(point.row as i64, point.col as i64)
}

#[test]
fn window_surface_snapshot_preserves_normalized_components() {
    let snapshot = WindowSurfaceSnapshot::new(
        SurfaceId::new(11, 17).expect("positive handles"),
        BufferLine::new(23).expect("positive buffer line"),
        0,
        4,
        ScreenCell::new(5, 7).expect("one-based window origin"),
        ViewportBounds::new(24, 80).expect("positive window size"),
    );

    assert_eq!(
        snapshot,
        WindowSurfaceSnapshot::new(
            SurfaceId::new(11, 17).expect("positive handles"),
            BufferLine::new(23).expect("positive buffer line"),
            0,
            4,
            ScreenCell::new(5, 7).expect("one-based window origin"),
            ViewportBounds::new(24, 80).expect("positive window size"),
        )
    );
}

#[test]
fn window_surface_snapshot_exposes_the_retained_surface_id() {
    let snapshot = WindowSurfaceSnapshot::new(
        SurfaceId::new(11, 17).expect("positive handles"),
        BufferLine::new(23).expect("positive buffer line"),
        0,
        4,
        ScreenCell::new(5, 7).expect("one-based window origin"),
        ViewportBounds::new(24, 80).expect("positive window size"),
    );

    assert_eq!(snapshot.id().window_handle(), 11);
    assert_eq!(snapshot.id().buffer_handle(), 17);
    assert_eq!(
        snapshot,
        WindowSurfaceSnapshot::new(
            SurfaceId::new(11, 17).expect("positive handles"),
            BufferLine::new(23).expect("positive buffer line"),
            0,
            4,
            ScreenCell::new(5, 7).expect("one-based window origin"),
            ViewportBounds::new(24, 80).expect("positive window size"),
        )
    );
}

#[test]
fn cursor_observation_retains_buffer_line_and_exactness() {
    assert_eq!(
        CursorObservation::new(
            BufferLine::new(12).expect("positive buffer line"),
            ObservedCell::Deferred(ScreenCell::new(4, 9).expect("one-based observed cell"))
        ),
        CursorObservation::new(
            BufferLine::new(12).expect("positive buffer line"),
            ObservedCell::Deferred(ScreenCell::new(4, 9).expect("one-based observed cell"))
        )
    );
}

#[test]
fn observed_cell_variants_preserve_exactness() {
    let cell = ScreenCell::new(4, 9).expect("one-based observed cell");

    assert_eq!(ObservedCell::Exact(cell), ObservedCell::Exact(cell));
    assert_eq!(ObservedCell::Deferred(cell), ObservedCell::Deferred(cell));
    assert_eq!(ObservedCell::Unavailable, ObservedCell::Unavailable);
    assert_ne!(ObservedCell::Exact(cell), ObservedCell::Deferred(cell));
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
    fn prop_buffer_line_validates_one_indexed_lines(line in -8_i64..8_i64) {
        let expected =
            (line >= 1).then(|| BufferLine::new(line).expect("one-based buffer lines should construct"));

        prop_assert_eq!(BufferLine::new(line), expected);
    }

    #[test]
    fn prop_buffer_line_round_trips_through_value_accessor(line in 1_i64..256_i64) {
        let buffer_line = BufferLine::new(line).expect("one-based buffer lines should construct");

        prop_assert_eq!(BufferLine::new(buffer_line.value()), Some(buffer_line));
    }

    #[test]
    fn prop_viewport_bounds_validate_positive_maxima(
        max_row in -8_i64..8_i64,
        max_col in -8_i64..8_i64,
    ) {
        let expected = (max_row >= 1 && max_col >= 1).then(|| {
            ViewportBounds::new(max_row, max_col)
                .expect("positive viewport bounds should construct")
        });

        prop_assert_eq!(ViewportBounds::new(max_row, max_col), expected);
    }

    #[test]
    fn prop_viewport_bounds_round_trip_through_accessors(
        max_row in 1_i64..256_i64,
        max_col in 1_i64..256_i64,
    ) {
        let bounds =
            ViewportBounds::new(max_row, max_col).expect("positive viewport bounds should construct");

        prop_assert_eq!(
            ViewportBounds::new(bounds.max_row(), bounds.max_col()),
            Some(bounds)
        );
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
    fn prop_screen_cell_from_host_point_rejects_fractional_and_invalid_values(
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
            ScreenCell::from_host_point(point),
            expected_host_screen_cell_from_point(point)
        );
    }

    #[test]
    fn prop_screen_cell_round_trips_through_render_point(
        row in 1_i64..256_i64,
        col in 1_i64..256_i64,
    ) {
        let cell = ScreenCell::new(row, col).expect("one-based rows and cols should construct");
        let point = RenderPoint::from(cell);

        prop_assert_eq!(
            point,
            RenderPoint {
                row: row as f64,
                col: col as f64,
            },
        );
        prop_assert_eq!(ScreenCell::from_rounded_point(point), Some(cell));
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

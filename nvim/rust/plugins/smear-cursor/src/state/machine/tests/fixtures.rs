use super::CursorLocation;
use super::CursorShape;
use crate::types::Particle;
use crate::types::Point;
use crate::types::StepOutput;
use proptest::prelude::*;

pub(super) fn point(row: f64, col: f64) -> Point {
    Point { row, col }
}

pub(super) fn location(
    window_handle: i64,
    buffer_handle: i64,
    row: i64,
    col: i64,
) -> CursorLocation {
    CursorLocation::new(window_handle, buffer_handle, row, col)
}

pub(super) fn default_shape() -> CursorShape {
    CursorShape::new(false, false)
}

pub(super) fn sample_step_output() -> StepOutput {
    StepOutput {
        current_corners: [point(4.0, 5.0); 4],
        velocity_corners: [point(1.0, 2.0); 4],
        spring_velocity_corners: [point(0.25, 0.5); 4],
        trail_elapsed_ms: [8.0, 8.0, 8.0, 8.0],
        particles: vec![Particle {
            position: point(6.0, 7.0),
            velocity: point(0.5, 0.25),
            lifetime: 0.75,
        }],
        previous_center: point(8.0, 9.0),
        index_head: 0,
        index_tail: 3,
        rng_state: 1234,
    }
}

pub(super) fn cursor_shape_strategy() -> BoxedStrategy<CursorShape> {
    prop_oneof![
        Just(CursorShape::new(false, false)),
        Just(CursorShape::new(true, false)),
        Just(CursorShape::new(false, true)),
    ]
    .boxed()
}

pub(super) fn cursor_location_strategy() -> BoxedStrategy<CursorLocation> {
    (
        any::<i16>(),
        any::<i16>(),
        -256_i64..256_i64,
        -256_i64..256_i64,
        -64_i64..64_i64,
        -64_i64..64_i64,
        -32_i64..32_i64,
        -32_i64..32_i64,
        1_i64..240_i64,
        1_i64..160_i64,
    )
        .prop_map(
            |(
                window_handle,
                buffer_handle,
                top_row,
                line,
                left_col,
                text_offset,
                window_row,
                window_col,
                window_width,
                window_height,
            )| {
                CursorLocation::new(
                    i64::from(window_handle),
                    i64::from(buffer_handle),
                    top_row,
                    line,
                )
                .with_viewport_columns(left_col, text_offset)
                .with_window_origin(window_row, window_col)
                .with_window_dimensions(window_width, window_height)
            },
        )
        .boxed()
}

pub(super) fn surface_changed(previous: Option<&CursorLocation>, next: &CursorLocation) -> bool {
    previous.is_some_and(|tracked| {
        tracked.window_handle != next.window_handle
            || tracked.buffer_handle != next.buffer_handle
            || tracked.window_dimensions_changed(next)
    })
}

pub(super) fn translate_corners(corners: [Point; 4], row_delta: f64, col_delta: f64) -> [Point; 4] {
    corners.map(|corner| point(corner.row + row_delta, corner.col + col_delta))
}

pub(super) fn row_bounds(corners: &[Point; 4]) -> (f64, f64) {
    let mut min_row = f64::INFINITY;
    let mut max_row = f64::NEG_INFINITY;
    for corner in corners {
        min_row = min_row.min(corner.row);
        max_row = max_row.max(corner.row);
    }

    (min_row, max_row)
}

pub(super) fn perturbed_location(location: &CursorLocation) -> CursorLocation {
    CursorLocation::new(
        location.window_handle,
        location.buffer_handle,
        location.top_row,
        location.line.saturating_add(1),
    )
    .with_viewport_columns(location.left_col, location.text_offset)
    .with_window_origin(location.window_row, location.window_col)
    .with_window_dimensions(location.window_width, location.window_height)
}

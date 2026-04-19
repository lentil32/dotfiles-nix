use super::CursorShape;
use super::TrackedCursor;
use crate::position::RenderPoint;
use crate::types::Particle;
use crate::types::StepOutput;
use proptest::prelude::*;

pub(super) fn point(row: f64, col: f64) -> RenderPoint {
    RenderPoint { row, col }
}

pub(super) fn location(
    window_handle: i64,
    buffer_handle: i64,
    row: i64,
    col: i64,
) -> TrackedCursor {
    TrackedCursor::fixture(window_handle, buffer_handle, row, col)
}

pub(super) fn default_shape() -> CursorShape {
    CursorShape::block()
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
        Just(CursorShape::block()),
        Just(CursorShape::vertical_bar()),
        Just(CursorShape::horizontal_bar()),
    ]
    .boxed()
}

pub(super) fn tracked_cursor_strategy() -> BoxedStrategy<TrackedCursor> {
    (
        1_i16..=i16::MAX,
        1_i16..=i16::MAX,
        1_i64..256_i64,
        1_i64..256_i64,
        0_i64..64_i64,
        0_i64..64_i64,
        1_i64..32_i64,
        1_i64..32_i64,
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
                TrackedCursor::fixture(
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

pub(super) fn translate_corners(
    corners: [RenderPoint; 4],
    row_delta: f64,
    col_delta: f64,
) -> [RenderPoint; 4] {
    corners.map(|corner| point(corner.row + row_delta, corner.col + col_delta))
}

pub(super) fn row_bounds(corners: &[RenderPoint; 4]) -> (f64, f64) {
    let mut min_row = f64::INFINITY;
    let mut max_row = f64::NEG_INFINITY;
    for corner in corners {
        min_row = min_row.min(corner.row);
        max_row = max_row.max(corner.row);
    }

    (min_row, max_row)
}

pub(super) fn perturbed_location(location: &TrackedCursor) -> TrackedCursor {
    TrackedCursor::fixture(
        location.window_handle(),
        location.buffer_handle(),
        location.surface().top_buffer_line().value(),
        location.buffer_line().value(),
    )
    .with_viewport_columns(
        i64::from(location.surface().left_col0()),
        i64::from(location.surface().text_offset0()),
    )
    .with_window_origin(
        location.surface().window_origin().row(),
        location.surface().window_origin().col(),
    )
    .with_window_dimensions(
        location.surface().window_size().max_col().saturating_add(1),
        location.surface().window_size().max_row(),
    )
}

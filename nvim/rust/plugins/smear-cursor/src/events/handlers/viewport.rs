use super::super::cursor::smear_outside_cmd_row;
use crate::core::effect::ObservationRuntimeContext;
use crate::core::runtime_reducer::ScrollShift;
use crate::events::surface::current_window_surface_snapshot_with;
use crate::host::BufferHandle;
use crate::host::CurrentEditorPort;
use crate::host::NeovimHost;
use crate::host::WindowSurfacePort;
use crate::host::api;
use crate::position::WindowSurfaceSnapshot;
use nvim_oxi::Result;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct SurfaceTranslationDelta {
    vertical_rows: Option<(i64, i64)>,
    horizontal_cols: i64,
    window_row_delta: i64,
    window_col_delta: i64,
}

fn snapshot_matches_buffer(snapshot: WindowSurfaceSnapshot, buffer: &api::Buffer) -> bool {
    snapshot.id().buffer_handle() == BufferHandle::from_buffer(buffer)
}

pub(crate) fn surface_for_ingress_fast_path_with_current_editor(
    current_host: &impl CurrentEditorPort,
    window: &api::Window,
    buffer: &api::Buffer,
) -> Option<WindowSurfaceSnapshot> {
    surface_for_ingress_fast_path_with_hosts(current_host, &NeovimHost, window, buffer)
}

fn surface_for_ingress_fast_path_with_hosts(
    current_host: &impl CurrentEditorPort,
    surface_host: &impl WindowSurfacePort,
    window: &api::Window,
    buffer: &api::Buffer,
) -> Option<WindowSurfaceSnapshot> {
    if !current_host.window_is_valid(window) || !current_host.buffer_is_valid(buffer) {
        return None;
    }

    let surface_snapshot = current_window_surface_snapshot_with(surface_host, window).ok()?;
    snapshot_matches_buffer(surface_snapshot, buffer).then_some(surface_snapshot)
}

fn line_index_1_to_0(row: i64) -> usize {
    let clamped = row.max(1).saturating_sub(1);
    usize::try_from(clamped).unwrap_or_default()
}

fn screen_distance(
    host: &impl WindowSurfacePort,
    window: &api::Window,
    viewport_height: i64,
    row_start: i64,
    row_end: i64,
) -> Result<f64> {
    let mut start = row_start;
    let mut end = row_end;
    let mut reversed = false;
    if start > end {
        std::mem::swap(&mut start, &mut end);
        reversed = true;
    }

    let distance = if end.saturating_sub(start) >= viewport_height {
        viewport_height.saturating_sub(1)
    } else {
        host.window_text_height_rows(window, line_index_1_to_0(start), line_index_1_to_0(end))
    };

    if reversed {
        Ok(-(distance as f64))
    } else {
        Ok(distance as f64)
    }
}

fn surface_translation_delta(
    previous_surface: WindowSurfaceSnapshot,
    current_surface: WindowSurfaceSnapshot,
) -> Option<SurfaceTranslationDelta> {
    if previous_surface.id() != current_surface.id() {
        return None;
    }
    if previous_surface.window_size() != current_surface.window_size() {
        return None;
    }

    let vertical_rows = (previous_surface.top_buffer_line() != current_surface.top_buffer_line())
        .then_some((
            previous_surface.top_buffer_line().value(),
            current_surface.top_buffer_line().value(),
        ));
    let horizontal_cols = i64::from(current_surface.left_col0())
        - i64::from(previous_surface.left_col0())
        - (i64::from(current_surface.text_offset0()) - i64::from(previous_surface.text_offset0()));
    let window_row_delta =
        current_surface.window_origin().row() - previous_surface.window_origin().row();
    let window_col_delta =
        current_surface.window_origin().col() - previous_surface.window_origin().col();
    if vertical_rows.is_none()
        && horizontal_cols == 0
        && window_row_delta == 0
        && window_col_delta == 0
    {
        return None;
    }

    Some(SurfaceTranslationDelta {
        vertical_rows,
        horizontal_cols,
        window_row_delta,
        window_col_delta,
    })
}

pub(super) fn maybe_scroll_shift_for_core_event(
    window: &api::Window,
    context: &ObservationRuntimeContext,
    current_surface: &WindowSurfaceSnapshot,
) -> Result<Option<ScrollShift>> {
    if !context.scroll_buffer_space() {
        return Ok(None);
    }
    let Some(previous_surface) = context.tracked_surface() else {
        return Ok(None);
    };
    if !smear_outside_cmd_row(&context.current_corners())? {
        return Ok(None);
    }
    let Some(delta) = surface_translation_delta(previous_surface, *current_surface) else {
        return Ok(None);
    };

    let viewport_row_shift = match delta.vertical_rows {
        Some((previous_top_row, current_top_row)) => screen_distance(
            &NeovimHost,
            window,
            current_surface.window_size().max_row(),
            previous_top_row,
            current_top_row,
        )?,
        None => 0.0,
    };
    let row_shift = viewport_row_shift - delta.window_row_delta as f64;
    let col_shift = delta.horizontal_cols as f64 - delta.window_col_delta as f64;
    if row_shift == 0.0 && col_shift == 0.0 {
        return Ok(None);
    }
    let window_row = current_surface.window_origin().row() as f64;
    let window_height = current_surface.window_size().max_row() as f64;
    let min_row = window_row;
    let max_row = min_row + window_height - 1.0;

    Ok(Some(ScrollShift {
        row_shift,
        col_shift,
        min_row,
        max_row,
    }))
}

#[cfg(test)]
mod tests {
    use super::SurfaceTranslationDelta;
    use super::screen_distance;
    use super::surface_translation_delta;
    use crate::host::FakeWindowSurfacePort;
    use crate::host::WindowSurfaceCall;
    use crate::host::api;
    use crate::state::TrackedCursor;
    use pretty_assertions::assert_eq;

    fn complete_surface_location(location: TrackedCursor) -> TrackedCursor {
        location.with_window_dimensions(80, 20)
    }

    fn assert_surface_translation_case(
        label: &str,
        previous: TrackedCursor,
        current: TrackedCursor,
        expected: SurfaceTranslationDelta,
    ) {
        let previous_surface = previous.surface();
        let current_surface = current.surface();
        assert_eq!(
            surface_translation_delta(previous_surface, current_surface),
            Some(expected),
            "{label}"
        );
    }

    #[test]
    fn screen_distance_reads_window_text_height_through_window_surface_port() {
        let host = FakeWindowSurfacePort::default();
        host.push_window_text_height_rows(7);

        let distance = screen_distance(&host, &api::Window::from(11), 20, 3, 6)
            .expect("screen distance should read through fake host");

        assert_eq!(
            (distance, host.calls(),),
            (
                7.0,
                vec![WindowSurfaceCall::WindowTextHeightRows {
                    window_handle: 11,
                    start_row: 2,
                    end_row: 5,
                }],
            )
        );
    }

    #[test]
    fn surface_translation_delta_detects_surface_motion_cases() {
        for (label, previous, current, expected) in [
            (
                "same-line vertical scroll",
                complete_surface_location(
                    TrackedCursor::fixture(10, 20, 4, 12)
                        .with_viewport_columns(3, 1)
                        .with_window_origin(3, 4),
                ),
                complete_surface_location(
                    TrackedCursor::fixture(10, 20, 6, 12)
                        .with_viewport_columns(3, 1)
                        .with_window_origin(3, 4),
                ),
                SurfaceTranslationDelta {
                    vertical_rows: Some((4, 6)),
                    horizontal_cols: 0,
                    window_row_delta: 0,
                    window_col_delta: 0,
                },
            ),
            (
                "same-line horizontal scroll",
                complete_surface_location(
                    TrackedCursor::fixture(10, 20, 4, 12)
                        .with_viewport_columns(2, 1)
                        .with_window_origin(3, 4),
                ),
                complete_surface_location(
                    TrackedCursor::fixture(10, 20, 4, 12)
                        .with_viewport_columns(5, 1)
                        .with_window_origin(3, 4),
                ),
                SurfaceTranslationDelta {
                    vertical_rows: None,
                    horizontal_cols: 3,
                    window_row_delta: 0,
                    window_col_delta: 0,
                },
            ),
            (
                "text offset change",
                complete_surface_location(
                    TrackedCursor::fixture(10, 20, 4, 12)
                        .with_viewport_columns(5, 2)
                        .with_window_origin(3, 4),
                ),
                complete_surface_location(
                    TrackedCursor::fixture(10, 20, 4, 12)
                        .with_viewport_columns(5, 4)
                        .with_window_origin(3, 4),
                ),
                SurfaceTranslationDelta {
                    vertical_rows: None,
                    horizontal_cols: -2,
                    window_row_delta: 0,
                    window_col_delta: 0,
                },
            ),
            (
                "window origin shift without viewport motion",
                complete_surface_location(
                    TrackedCursor::fixture(10, 20, 4, 12)
                        .with_viewport_columns(2, 1)
                        .with_window_origin(3, 4),
                ),
                complete_surface_location(
                    TrackedCursor::fixture(10, 20, 4, 12)
                        .with_viewport_columns(2, 1)
                        .with_window_origin(5, 7),
                ),
                SurfaceTranslationDelta {
                    vertical_rows: None,
                    horizontal_cols: 0,
                    window_row_delta: 2,
                    window_col_delta: 3,
                },
            ),
        ] {
            assert_surface_translation_case(label, previous, current, expected);
        }
    }

    #[test]
    fn surface_translation_delta_ignores_window_dimension_changes() {
        let previous = TrackedCursor::fixture(10, 20, 4, 12)
            .with_viewport_columns(2, 1)
            .with_window_origin(3, 4)
            .with_window_dimensions(80, 20);
        let current = TrackedCursor::fixture(10, 20, 4, 12)
            .with_viewport_columns(2, 1)
            .with_window_origin(3, 4)
            .with_window_dimensions(72, 20);

        assert_eq!(
            surface_translation_delta(previous.surface(), current.surface(),),
            None,
        );
    }

    #[test]
    fn surface_translation_delta_ignores_cursor_motion_without_surface_motion() {
        let previous = complete_surface_location(
            TrackedCursor::fixture(10, 20, 4, 12)
                .with_viewport_columns(2, 1)
                .with_window_origin(3, 4),
        );
        let current = complete_surface_location(
            TrackedCursor::fixture(10, 20, 4, 13)
                .with_viewport_columns(2, 1)
                .with_window_origin(3, 4),
        );

        assert_eq!(
            surface_translation_delta(previous.surface(), current.surface(),),
            None,
        );
    }
}

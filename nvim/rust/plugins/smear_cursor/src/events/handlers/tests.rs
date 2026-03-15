use super::{EventSource, select_core_event_source};
use crate::state::{CursorLocation, CursorShape, RuntimeState};
use crate::types::Point;

fn location(window_handle: i64, buffer_handle: i64) -> CursorLocation {
    CursorLocation::new(window_handle, buffer_handle, 1, 1)
}

#[test]
fn cmdline_mode_always_routes_to_external_source() {
    let mut state = RuntimeState::default();
    let tracked = location(10, 20);
    state.initialize_cursor(
        Point { row: 5.0, col: 6.0 },
        CursorShape::new(false, false),
        7,
        &tracked,
    );
    state.start_animation();

    let source = select_core_event_source(
        "c",
        &state,
        Some(Point {
            row: 5.0,
            col: 12.0,
        }),
        &tracked,
    );
    assert_eq!(source, EventSource::External);
}

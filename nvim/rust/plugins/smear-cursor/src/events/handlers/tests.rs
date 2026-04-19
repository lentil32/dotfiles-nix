use super::EventSource;
use super::select_core_event_source;
use crate::core::runtime_reducer::MotionTarget;
use crate::core::state::SemanticEvent;
use crate::position::RenderPoint;
use crate::position::ScreenCell;
use crate::state::CursorShape;
use crate::state::RuntimeState;
use crate::state::TrackedCursor;

fn location(window_handle: i64, buffer_handle: i64) -> TrackedCursor {
    TrackedCursor::fixture(window_handle, buffer_handle, 1, 1)
}

#[test]
fn cmdline_mode_always_routes_to_external_source() {
    let mut state = RuntimeState::default();
    let tracked = location(10, 20);
    state.initialize_cursor(
        RenderPoint { row: 5.0, col: 6.0 },
        CursorShape::block(),
        7,
        &tracked,
    );
    state.start_animation();

    let source = select_core_event_source(
        "c",
        &state,
        SemanticEvent::FrameCommitted,
        MotionTarget::Available(ScreenCell::new(5, 12).expect("positive motion target")),
        &tracked,
    );
    assert_eq!(source, EventSource::External);
}

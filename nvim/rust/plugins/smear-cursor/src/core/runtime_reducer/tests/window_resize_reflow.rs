use super::*;
use pretty_assertions::assert_eq;

#[test]
fn window_resize_clears_inflight_smear_and_classifies_as_surface_retarget() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.max_simulation_steps_per_frame = 0;

    let previous_location = TrackedCursor::fixture(10, 20, 4, 12).with_window_dimensions(80, 24);
    state.initialize_cursor(
        RenderPoint {
            row: 15.0,
            col: 18.0,
        },
        CursorShape::block(),
        7,
        &previous_location,
    );
    replace_target_preserving_tracking(
        &mut state,
        RenderPoint {
            row: 15.0,
            col: 24.0,
        },
        CursorShape::block(),
    );
    state.start_animation_towards_target();
    state.set_last_tick_ms(Some(100.0));
    let baseline_epoch = state.retarget_epoch();

    let current_location = TrackedCursor::fixture(10, 20, 4, 12).with_window_dimensions(72, 24);
    let transition = reduce_cursor_event(
        &mut state,
        "n",
        CursorEventContext {
            row: 15.0,
            col: 24.0,
            now_ms: 116.0,
            seed: 9,
            tracked_cursor: current_location.clone(),
            scroll_shift: None,
            semantic_event: SemanticEvent::ViewportOrWindowMoved,
        },
        EventSource::External,
    );

    assert!(matches!(render_action(&transition), RenderAction::ClearAll));
    assert_eq!(
        render_cleanup_action(&transition),
        RenderCleanupAction::Schedule
    );
    assert_eq!(transition.motion_class, MotionClass::SurfaceRetarget);
    assert!(!state.is_animating());
    assert_eq!(state.tracked_cursor(), Some(current_location));
    assert_eq!(
        state.target_position(),
        RenderPoint {
            row: 15.0,
            col: 24.0
        }
    );
    assert_eq!(state.retarget_epoch(), baseline_epoch.wrapping_add(1));
}

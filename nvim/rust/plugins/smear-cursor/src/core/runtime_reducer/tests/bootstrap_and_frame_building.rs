use super::*;

#[test]
fn disabled_state_reduces_to_clear_all() {
    let mut state = RuntimeState::default();
    state.set_enabled(false);
    state.start_animation();

    let effects = reduce_cursor_event(&mut state, "n", event(3.0, 8.0), EventSource::External);
    assert!(matches!(render_action(&effects), RenderAction::ClearAll));
    assert_eq!(
        render_cleanup_action(&effects),
        RenderCleanupAction::Invalidate
    );
    assert!(!state.is_animating());
}

#[test]
fn first_external_event_bootstraps_frame_cleanup_allocation_and_idle_state() {
    let (state, transition) = initialized_runtime("n", |_| {});

    assert!(matches!(render_action(&transition), RenderAction::Draw(_)));
    assert_eq!(
        render_cleanup_action(&transition),
        RenderCleanupAction::Schedule
    );
    assert_eq!(
        render_allocation_policy(&transition),
        RenderAllocationPolicy::BootstrapIfPoolEmpty
    );
    assert!(state.is_initialized());
    assert!(!state.is_animating());
}

#[test]
fn build_render_frame_preserves_core_cursor_geometry() {
    let mut state = RuntimeState::default();
    let location = CursorLocation::new(10, 20, 1, 1);
    let shape = CursorShape::new(false, false);
    let position = Point { row: 5.0, col: 7.0 };
    state.initialize_cursor(position, shape, 3, &location);

    let frame = crate::core::runtime_reducer::build_render_frame(
        &state,
        "n",
        state.current_corners(),
        Vec::new(),
        0,
        state.target_position(),
        false,
    );

    assert_eq!(frame.corners, state.current_corners());
    assert!(frame.step_samples.is_empty());
    assert_eq!(frame.target, state.target_position());
    assert_eq!(frame.target_corners, state.target_corners());
}

#[test]
fn draw_frame_exports_each_executed_simulation_step() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.time_interval = 16.0;
    state.config.simulation_hz = 240.0;
    state.config.max_simulation_steps_per_frame = 16;

    let _ = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 6.0, 100.0),
        EventSource::External,
    );
    let effects = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 16.0, 108.0),
        EventSource::External,
    );
    let frame = draw_frame(&effects).expect("external retarget should draw");

    assert_eq!(frame.step_samples.len(), 3);
    assert!(
        frame.step_samples.iter().all(|sample| sample.dt_ms > 0.0),
        "each exported simulation sample should carry a positive fixed-step dt"
    );
    assert_eq!(
        frame.step_samples.last().map(|sample| sample.corners),
        Some(frame.corners)
    );
}

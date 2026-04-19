use super::*;
use proptest::collection::vec;

proptest! {
    #![proptest_config(stateful_config())]

    #[test]
    fn prop_lifecycle_flags_follow_operation_sequences(
        operations in vec(lifecycle_sequence_operation_strategy(), 1..24)
    ) {
        let mut state = RuntimeState::default();

        for operation in operations {
            let (expected_initialized, expected_animating) =
                expected_lifecycle_flags(&state, &operation);

            apply_lifecycle_sequence_operation(&mut state, &operation);

            prop_assert_eq!(
                state.is_initialized(),
                expected_initialized,
                "operation={:?}",
                operation
            );
            prop_assert_eq!(
                state.is_animating(),
                expected_animating,
                "operation={:?}",
                operation
            );
        }
    }
}

#[test]
fn start_animation_towards_target_seeds_velocity_and_enters_animating_phase() {
    let mut state = RuntimeState::default();
    state.config.anticipation = 0.42;
    state.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &location(1, 2, 3, 4));
    replace_target_preserving_tracking(&mut state, point(8.0, 9.0), default_shape());

    let expected_velocity = initial_velocity(
        &state.current_corners(),
        &state.target_corners(),
        state.config.anticipation,
    );
    state.start_animation_towards_target();

    assert_eq!(state.velocity_corners(), expected_velocity);
    assert!(state.is_animating());
}

#[test]
fn start_animation_discards_settling_window_and_arms_motion_clock() {
    let mut state = RuntimeState::default();
    let tracked = location(1, 2, 3, 4);
    state.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    state.begin_settling(point(8.0, 9.0), default_shape(), &tracked, 100.0);

    state.start_animation();

    assert!(state.is_animating());
    assert!(state.settling_window().is_none());
    assert!(state.has_motion_clock());
}

#[test]
fn stop_animation_drops_the_motion_clock() {
    let mut state = RuntimeState::default();

    state.start_animation();
    state.stop_animation();

    assert!(!state.is_animating());
    assert!(!state.has_motion_clock());
}

#[test]
fn start_tail_drain_transitions_follow_requested_step_count() {
    for (label, drain_steps, expected_state) in [
        (
            "zero-step drain keeps the runtime idle",
            0,
            (false, 0, false),
        ),
        (
            "positive-step drain enters draining with owned clock state",
            3,
            (true, 3, true),
        ),
    ] {
        let mut state = RuntimeState::default();
        state.mark_initialized();

        state.start_tail_drain(drain_steps);

        assert_eq!(
            (
                state.is_draining(),
                state.drain_steps_remaining(),
                state.has_motion_clock(),
            ),
            expected_state,
            "{label}"
        );
    }
}

#[test]
fn reset_transient_state_restores_default_transient_fields() {
    let mut state = RuntimeState::default();
    replace_target_preserving_tracking(&mut state, point(4.0, 9.0), default_shape());
    replace_target_with_tracking(
        &mut state,
        point(4.0, 9.0),
        default_shape(),
        &location(11, 22, 33, 44),
    );
    state.set_color_at_cursor(Some(0x00FF_FFFF));
    state.start_animation();
    state.set_last_tick_ms(Some(99.0));
    let mut expected = state.clone();
    expected.transient = Default::default();

    state.reset_transient_state();

    pretty_assertions::assert_eq!(state.semantic_view(), expected.semantic_view());
}

#[test]
fn cmdline_boundary_tracking_only_reports_known_mode_crossings() {
    let mut state = RuntimeState::default();

    for (label, current_is_cmdline, expected_boundary) in [
        (
            "unknown mode memory does not invent a boundary",
            true,
            false,
        ),
        (
            "repeating cmdline mode does not cross the boundary",
            true,
            false,
        ),
        ("leaving cmdline mode crosses the boundary", false, true),
        (
            "repeating non-cmdline mode does not cross the boundary",
            false,
            false,
        ),
        ("re-entering cmdline mode crosses the boundary", true, true),
    ] {
        assert_eq!(
            state.crossed_cmdline_boundary(current_is_cmdline),
            expected_boundary,
            "{label}"
        );
        state.record_observed_mode(current_is_cmdline);
    }
}

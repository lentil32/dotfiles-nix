use super::*;
use proptest::collection::vec;

fn runtime_with_retained_motion_and_purgeable_storage() -> RuntimeState {
    let tracked = location(11, 22, 33, 44);
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 27.0;
    state.config.hide_target_hack = false;
    state.commit_runtime_config_update();
    state.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    replace_target_preserving_tracking(&mut state, point(8.0, 9.0), default_shape());
    state.start_animation_towards_target();
    state.set_color_at_cursor(Some(0x00AB_CDEF));
    state.record_observed_mode(/*current_is_cmdline*/ true);
    state.set_last_tick_ms(Some(99.0));
    state.apply_step_output(sample_step_output());
    let _ = state.shared_particle_screen_cells();
    state.reclaim_preview_particles_scratch(Vec::with_capacity(8));
    let mut scratch = state.take_render_step_samples_scratch();
    scratch.reserve(4);
    scratch.push(RenderStepSample::new([point(13.0, 14.0); 4], 16.0));
    state.reclaim_render_step_samples_scratch(scratch);
    state
}

fn runtime_after_cold_clear(source: &RuntimeState) -> RuntimeState {
    let mut expected = source.clone();
    expected.clear_initialization();
    expected.reset_transient_state();
    expected.clear_particles();
    expected.caches = Default::default();
    expected
}

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

        state.start_tail_drain(drain_steps, 100.0);

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
fn animation_clock_sample_reports_discontinuity_for_large_gaps() {
    let mut state = RuntimeState::default();
    state.config.time_interval = 16.0;
    state.config.simulation_hz = 120.0;
    state.config.max_simulation_steps_per_frame = 16;
    state.start_animation();
    state.set_last_tick_ms(Some(100.0));

    let sample = state.take_animation_clock_sample(2_500.0, 16.0);

    pretty_assertions::assert_eq!(sample, AnimationClockSample::Discontinuity);
    pretty_assertions::assert_eq!(state.last_tick_ms(), Some(2_500.0));
}

#[test]
fn push_simulation_elapsed_caps_motion_clock_debt_to_the_catch_up_budget() {
    let mut state = RuntimeState::default();
    state.config.time_interval = 16.0;
    state.config.simulation_hz = 120.0;
    state.config.max_simulation_steps_per_frame = 4;
    state.start_animation();

    let catch_up_budget_ms = state.animation_clock_catch_up_budget_ms();
    state.push_simulation_elapsed(catch_up_budget_ms * 4.0);

    pretty_assertions::assert_eq!(state.simulation_accumulator_ms(), catch_up_budget_ms);
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
fn clear_runtime_state_cold_resets_motion_and_releases_retained_storage() {
    let mut state = runtime_with_retained_motion_and_purgeable_storage();
    let expected = runtime_after_cold_clear(&state);

    assert!(state.preview_particles_scratch_capacity() > 0);
    assert!(state.render_step_samples_scratch_capacity() > 0);
    assert!(state.particle_aggregation_scratch_index_capacity() > 0);
    assert!(state.particle_aggregation_scratch_cells_capacity() > 0);
    assert!(state.particle_aggregation_scratch_screen_cells_capacity() > 0);
    assert!(state.has_cached_aggregated_particle_cells());
    assert!(state.has_cached_particle_screen_cells());

    state.clear_runtime_state();

    pretty_assertions::assert_eq!(state.semantic_view(), expected.semantic_view());
    assert_eq!(state.preview_particles_scratch_capacity(), 0);
    assert_eq!(state.render_step_samples_scratch_capacity(), 0);
    assert_eq!(state.particle_aggregation_scratch_index_capacity(), 0);
    assert_eq!(state.particle_aggregation_scratch_cells_capacity(), 0);
    assert_eq!(
        state.particle_aggregation_scratch_screen_cells_capacity(),
        0
    );
    assert!(!state.has_cached_aggregated_particle_cells());
    assert!(!state.has_cached_particle_screen_cells());
}

#[test]
fn disable_cold_resets_runtime_and_marks_plugin_disabled() {
    let mut state = runtime_with_retained_motion_and_purgeable_storage();
    let mut expected = runtime_after_cold_clear(&state);
    expected.set_enabled(false);

    state.disable();

    pretty_assertions::assert_eq!(state.semantic_view(), expected.semantic_view());
    assert_eq!(state.preview_particles_scratch_capacity(), 0);
    assert_eq!(state.render_step_samples_scratch_capacity(), 0);
    assert_eq!(state.particle_aggregation_scratch_index_capacity(), 0);
    assert_eq!(state.particle_aggregation_scratch_cells_capacity(), 0);
    assert_eq!(
        state.particle_aggregation_scratch_screen_cells_capacity(),
        0
    );
    assert!(!state.has_cached_aggregated_particle_cells());
    assert!(!state.has_cached_particle_screen_cells());
    assert!(!state.is_enabled());
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

use super::{CursorLocation, CursorShape, RuntimeState};
use crate::animation::{center, corners_for_cursor, initial_velocity};
use crate::types::{Particle, Point, StepOutput};

fn point(row: f64, col: f64) -> Point {
    Point { row, col }
}

fn location(window_handle: i64, buffer_handle: i64, row: i64, col: i64) -> CursorLocation {
    CursorLocation::new(window_handle, buffer_handle, row, col)
}

fn default_shape() -> CursorShape {
    CursorShape::new(false, false)
}

fn sample_step_output() -> StepOutput {
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

mod animation_lifecycle {
    use super::{RuntimeState, default_shape, initial_velocity, location, point};

    #[test]
    fn mark_initialized_sets_initialized_without_starting_animation() {
        let mut state = RuntimeState::default();

        state.mark_initialized();

        assert!(state.is_initialized());
        assert!(!state.is_animating());
    }

    #[test]
    fn start_animation_sets_initialized_and_animating() {
        let mut state = RuntimeState::default();

        state.start_animation();

        assert!(state.is_initialized());
        assert!(state.is_animating());
    }

    #[test]
    fn stop_animation_clears_running_flag_without_clearing_initialization() {
        let mut state = RuntimeState::default();
        state.start_animation();

        state.stop_animation();

        assert!(state.is_initialized());
        assert!(!state.is_animating());
    }

    #[test]
    fn clear_initialization_returns_state_to_uninitialized() {
        let mut state = RuntimeState::default();
        state.mark_initialized();

        state.clear_initialization();

        assert!(!state.is_initialized());
        assert!(!state.is_animating());
    }

    #[test]
    fn start_animation_towards_target_uses_target_delta_to_seed_velocity() {
        let mut state = RuntimeState::default();
        state.config.anticipation = 0.42;
        state.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &location(1, 2, 3, 4));
        state.set_target(point(8.0, 9.0), default_shape());

        let expected_velocity = initial_velocity(
            &state.current_corners(),
            &state.target_corners(),
            state.config.anticipation,
        );
        state.start_animation_towards_target();

        assert_eq!(state.velocity_corners(), expected_velocity);
    }

    #[test]
    fn start_animation_towards_target_enters_animating_phase() {
        let mut state = RuntimeState::default();
        state.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &location(1, 2, 3, 4));
        state.set_target(point(8.0, 9.0), default_shape());

        state.start_animation_towards_target();

        assert!(state.is_animating());
    }

    #[test]
    fn start_animation_discards_settling_payload_and_arms_motion_clock() {
        let mut state = RuntimeState::default();
        let tracked = location(1, 2, 3, 4);
        state.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
        state.begin_settling(point(8.0, 9.0), default_shape(), &tracked, 100.0);

        state.start_animation();

        assert!(state.is_animating());
        assert!(state.pending_target().is_none());
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
    fn start_tail_drain_with_zero_steps_keeps_the_runtime_idle() {
        let mut state = RuntimeState::default();
        state.mark_initialized();

        state.start_tail_drain(0);

        assert!(!state.is_draining());
        assert_eq!(state.drain_steps_remaining(), 0);
        assert!(!state.has_motion_clock());
    }

    #[test]
    fn start_tail_drain_with_steps_enters_draining_with_phase_owned_state() {
        let mut state = RuntimeState::default();
        state.mark_initialized();

        state.start_tail_drain(3);

        assert!(state.is_draining());
        assert_eq!(state.drain_steps_remaining(), 3);
        assert!(state.has_motion_clock());
    }
}

mod transient_state {
    use super::{RuntimeState, default_shape, location, point};

    #[test]
    fn reset_transient_state_clears_target_position() {
        let mut state = RuntimeState::default();
        state.set_target(point(4.0, 9.0), default_shape());
        state.update_tracking(&location(11, 22, 33, 44));
        state.set_color_at_cursor(Some("#ffffff".to_string()));
        state.set_last_tick_ms(Some(99.0));

        state.reset_transient_state();

        assert_eq!(state.target_position(), point(0.0, 0.0));
    }

    #[test]
    fn reset_transient_state_clears_tracking() {
        let mut state = RuntimeState::default();
        state.update_tracking(&location(11, 22, 33, 44));

        state.reset_transient_state();

        assert_eq!(state.tracked_location(), None);
    }

    #[test]
    fn reset_transient_state_clears_cursor_color() {
        let mut state = RuntimeState::default();
        state.set_color_at_cursor(Some("#ffffff".to_string()));

        state.reset_transient_state();

        assert_eq!(state.color_at_cursor(), None);
    }

    #[test]
    fn reset_transient_state_clears_last_tick() {
        let mut state = RuntimeState::default();
        state.set_last_tick_ms(Some(99.0));

        state.reset_transient_state();

        assert_eq!(state.last_tick_ms(), None);
    }
}

mod retarget_epoch_rules {
    use super::{RuntimeState, default_shape, location, point};

    #[test]
    fn initialize_cursor_advances_retarget_epoch_from_default() {
        let mut state = RuntimeState::default();

        state.initialize_cursor(point(5.0, 6.0), default_shape(), 7, &location(1, 2, 3, 4));

        assert!(state.retarget_epoch() > 0);
    }

    #[test]
    fn set_target_keeps_epoch_when_target_position_is_unchanged() {
        let mut state = RuntimeState::default();
        let location = location(1, 2, 3, 4);
        let origin = point(5.0, 6.0);
        state.initialize_cursor(origin, default_shape(), 7, &location);
        let baseline_epoch = state.retarget_epoch();

        state.set_target(origin, default_shape());

        assert_eq!(state.retarget_epoch(), baseline_epoch);
    }

    #[test]
    fn set_target_advances_epoch_when_target_position_changes() {
        let mut state = RuntimeState::default();
        let location = location(1, 2, 3, 4);
        let origin = point(5.0, 6.0);
        state.initialize_cursor(origin, default_shape(), 7, &location);
        let baseline_epoch = state.retarget_epoch();

        state.set_target(point(5.0, 9.0), default_shape());

        assert_eq!(state.retarget_epoch(), baseline_epoch.wrapping_add(1));
    }

    #[test]
    fn jump_and_stop_animation_keeps_epoch_when_target_position_is_unchanged() {
        let mut state = RuntimeState::default();
        let location = location(1, 2, 3, 4);
        let target = point(5.0, 9.0);
        state.initialize_cursor(point(5.0, 6.0), default_shape(), 7, &location);
        state.set_target(target, default_shape());
        let baseline_epoch = state.retarget_epoch();

        state.jump_and_stop_animation(target, default_shape(), &location);

        assert_eq!(state.retarget_epoch(), baseline_epoch);
    }

    #[test]
    fn update_tracking_keeps_epoch_when_surface_is_unchanged() {
        let mut state = RuntimeState::default();
        let tracked = location(1, 2, 3, 4);
        state.initialize_cursor(point(5.0, 6.0), default_shape(), 7, &tracked);
        let baseline_epoch = state.retarget_epoch();

        state.update_tracking(&tracked);

        assert_eq!(state.retarget_epoch(), baseline_epoch);
    }

    #[test]
    fn update_tracking_advances_epoch_when_window_changes() {
        let mut state = RuntimeState::default();
        let tracked = location(1, 2, 3, 4);
        state.initialize_cursor(point(5.0, 6.0), default_shape(), 7, &tracked);
        let baseline_epoch = state.retarget_epoch();

        state.update_tracking(&location(9, 2, 3, 4));

        assert_eq!(state.retarget_epoch(), baseline_epoch.wrapping_add(1));
    }

    #[test]
    fn update_tracking_advances_epoch_when_buffer_changes() {
        let mut state = RuntimeState::default();
        let tracked = location(1, 2, 3, 4);
        state.initialize_cursor(point(5.0, 6.0), default_shape(), 7, &tracked);
        state.update_tracking(&location(9, 2, 3, 4));
        let baseline_epoch = state.retarget_epoch();

        state.update_tracking(&location(9, 88, 3, 4));

        assert_eq!(state.retarget_epoch(), baseline_epoch.wrapping_add(1));
    }
}

mod trail_stroke_rules {
    use super::{RuntimeState, default_shape, location, point};

    #[test]
    fn initialize_cursor_starts_the_first_trail_stroke() {
        let mut state = RuntimeState::default();

        state.initialize_cursor(point(5.0, 6.0), default_shape(), 7, &location(1, 2, 3, 4));

        assert!(state.trail_stroke_id().value() > 0);
    }

    #[test]
    fn set_target_keeps_trail_stroke_for_plain_retarget() {
        let mut state = RuntimeState::default();
        let location = location(1, 2, 3, 4);
        state.initialize_cursor(point(5.0, 6.0), default_shape(), 7, &location);
        let baseline_stroke = state.trail_stroke_id();

        state.set_target(point(5.0, 9.0), default_shape());

        assert_eq!(state.trail_stroke_id(), baseline_stroke);
    }

    #[test]
    fn jump_preserving_motion_starts_a_new_trail_stroke() {
        let mut state = RuntimeState::default();
        let location = location(1, 2, 3, 4);
        state.initialize_cursor(point(5.0, 6.0), default_shape(), 7, &location);
        let baseline_stroke = state.trail_stroke_id();

        state.jump_preserving_motion(point(8.0, 9.0), default_shape(), &location);

        assert!(state.trail_stroke_id() > baseline_stroke);
    }

    #[test]
    fn jump_and_stop_animation_starts_a_new_trail_stroke() {
        let mut state = RuntimeState::default();
        let location = location(1, 2, 3, 4);
        state.initialize_cursor(point(5.0, 6.0), default_shape(), 7, &location);
        state.jump_preserving_motion(point(8.0, 9.0), default_shape(), &location);
        let baseline_stroke = state.trail_stroke_id();

        state.jump_and_stop_animation(point(9.0, 9.0), default_shape(), &location);

        assert!(state.trail_stroke_id() > baseline_stroke);
    }
}

mod cursor_transition_operations {
    use super::{
        Particle, Point, RuntimeState, center, corners_for_cursor, default_shape, location, point,
    };

    #[test]
    fn initialize_cursor_marks_state_as_initialized() {
        let mut state = RuntimeState::default();

        state.initialize_cursor(
            point(8.0, 9.0),
            default_shape(),
            123,
            &location(10, 20, 30, 40),
        );

        assert!(state.is_initialized());
    }

    #[test]
    fn initialize_cursor_stops_animation() {
        let mut state = RuntimeState::default();
        state.start_animation();

        state.initialize_cursor(
            point(8.0, 9.0),
            default_shape(),
            123,
            &location(10, 20, 30, 40),
        );

        assert!(!state.is_animating());
    }

    #[test]
    fn initialize_cursor_syncs_current_cursor_geometry_to_target() {
        let mut state = RuntimeState::default();
        let position = point(8.0, 9.0);
        let expected_corners = corners_for_cursor(position.row, position.col, false, false);

        state.initialize_cursor(position, default_shape(), 123, &location(10, 20, 30, 40));

        assert_eq!(state.current_corners(), expected_corners);
    }

    #[test]
    fn initialize_cursor_syncs_target_cursor_geometry_to_current() {
        let mut state = RuntimeState::default();
        let position = point(8.0, 9.0);
        let expected_corners = corners_for_cursor(position.row, position.col, false, false);

        state.initialize_cursor(position, default_shape(), 123, &location(10, 20, 30, 40));

        assert_eq!(state.target_corners(), expected_corners);
    }

    #[test]
    fn initialize_cursor_clears_velocity_corners() {
        let mut state = RuntimeState::default();
        state.velocity_corners = [point(1.0, 2.0); 4];

        state.initialize_cursor(
            point(8.0, 9.0),
            default_shape(),
            123,
            &location(10, 20, 30, 40),
        );

        assert_eq!(state.velocity_corners(), [Point::ZERO; 4]);
    }

    #[test]
    fn initialize_cursor_clears_particles() {
        let mut state = RuntimeState::default();
        state.particles.push(Particle {
            position: point(0.0, 0.0),
            velocity: point(0.0, 0.0),
            lifetime: 1.0,
        });

        state.initialize_cursor(
            point(8.0, 9.0),
            default_shape(),
            123,
            &location(10, 20, 30, 40),
        );

        assert!(state.particles().is_empty());
    }

    #[test]
    fn initialize_cursor_updates_rng_state() {
        let mut state = RuntimeState::default();

        state.initialize_cursor(
            point(8.0, 9.0),
            default_shape(),
            123,
            &location(10, 20, 30, 40),
        );

        assert_eq!(state.rng_state(), 123);
    }

    #[test]
    fn initialize_cursor_tracks_cursor_location() {
        let mut state = RuntimeState::default();
        let tracked = location(10, 20, 30, 40);

        state.initialize_cursor(point(8.0, 9.0), default_shape(), 123, &tracked);

        assert_eq!(state.tracked_location(), Some(tracked));
    }

    #[test]
    fn jump_preserving_motion_keeps_animation_running() {
        let mut state = RuntimeState::default();
        state.mark_initialized();
        state.start_animation();
        state.velocity_corners = [point(2.0, 3.0); 4];

        state.jump_preserving_motion(point(4.0, 5.0), default_shape(), &location(1, 2, 3, 4));

        assert!(state.is_animating());
    }

    #[test]
    fn jump_preserving_motion_preserves_velocity_corners() {
        let mut state = RuntimeState::default();
        state.mark_initialized();
        state.start_animation();
        state.velocity_corners = [point(2.0, 3.0); 4];

        state.jump_preserving_motion(point(4.0, 5.0), default_shape(), &location(1, 2, 3, 4));

        assert_eq!(state.velocity_corners, [point(2.0, 3.0); 4]);
    }

    #[test]
    fn sync_to_current_cursor_updates_target_position() {
        let mut state = RuntimeState::default();
        let target = point(6.0, 7.0);

        state.sync_to_current_cursor(target, default_shape(), &location(1, 2, 3, 4));

        assert_eq!(state.target_position(), target);
    }

    #[test]
    fn sync_to_current_cursor_updates_tracking() {
        let mut state = RuntimeState::default();
        let tracked = location(1, 2, 3, 4);

        state.sync_to_current_cursor(point(6.0, 7.0), default_shape(), &tracked);

        assert_eq!(state.tracked_location(), Some(tracked));
    }

    #[test]
    fn apply_scroll_shift_clamps_cursor_row_to_minimum() {
        let mut state = RuntimeState::default();
        state.initialize_cursor(point(5.0, 9.0), default_shape(), 11, &location(1, 2, 3, 4));

        state.apply_scroll_shift(10.0, 1.0, 99.0, false, false);

        assert_eq!(
            state.current_corners(),
            corners_for_cursor(1.0, 9.0, false, false)
        );
    }

    #[test]
    fn apply_scroll_shift_updates_previous_center_to_shifted_cursor_geometry() {
        let mut state = RuntimeState::default();
        state.initialize_cursor(point(5.0, 9.0), default_shape(), 11, &location(1, 2, 3, 4));
        let shifted_corners = corners_for_cursor(1.0, 9.0, false, false);

        state.apply_scroll_shift(10.0, 1.0, 99.0, false, false);

        assert_eq!(state.previous_center(), center(&shifted_corners));
    }

    #[test]
    fn apply_scroll_shift_moves_particle_rows_by_scroll_delta() {
        let mut state = RuntimeState::default();
        state.initialize_cursor(point(5.0, 9.0), default_shape(), 11, &location(1, 2, 3, 4));
        state.particles.push(Particle {
            position: point(10.0, 2.0),
            velocity: point(0.0, 0.0),
            lifetime: 1.0,
        });

        state.apply_scroll_shift(10.0, 1.0, 99.0, false, false);

        assert_eq!(state.particles()[0].position.row, 0.0);
    }
}

mod step_output_application {
    use super::{RuntimeState, sample_step_output};

    #[test]
    fn apply_step_output_replaces_current_corners() {
        let mut state = RuntimeState::default();
        let output = sample_step_output();
        let expected = output.current_corners;

        state.apply_step_output(output);

        assert_eq!(state.current_corners(), expected);
    }

    #[test]
    fn apply_step_output_replaces_velocity_corners() {
        let mut state = RuntimeState::default();
        let output = sample_step_output();
        let expected = output.velocity_corners;

        state.apply_step_output(output);

        assert_eq!(state.velocity_corners(), expected);
    }

    #[test]
    fn apply_step_output_replaces_spring_velocity_corners() {
        let mut state = RuntimeState::default();
        let output = sample_step_output();
        let expected = output.spring_velocity_corners;

        state.apply_step_output(output);

        assert_eq!(state.spring_velocity_corners(), expected);
    }

    #[test]
    fn apply_step_output_replaces_trail_elapsed_ms() {
        let mut state = RuntimeState::default();
        let output = sample_step_output();
        let expected = output.trail_elapsed_ms;

        state.apply_step_output(output);

        assert_eq!(state.trail_elapsed_ms(), expected);
    }

    #[test]
    fn apply_step_output_replaces_previous_center() {
        let mut state = RuntimeState::default();
        let output = sample_step_output();
        let expected = output.previous_center;

        state.apply_step_output(output);

        assert_eq!(state.previous_center(), expected);
    }

    #[test]
    fn apply_step_output_replaces_rng_state() {
        let mut state = RuntimeState::default();
        let output = sample_step_output();
        let expected = output.rng_state;

        state.apply_step_output(output);

        assert_eq!(state.rng_state(), expected);
    }

    #[test]
    fn apply_step_output_replaces_particles() {
        let mut state = RuntimeState::default();
        let expected = sample_step_output().particles;
        let output = sample_step_output();

        state.apply_step_output(output);

        assert_eq!(state.particles(), expected.as_slice());
    }
}

mod settling_rules {
    use super::{RuntimeState, default_shape, location, point};

    #[test]
    fn should_promote_settled_target_returns_false_before_deadline() {
        let mut state = RuntimeState::default();
        state.config.delay_event_to_smear = 25.0;
        let tracked = location(1, 2, 3, 4);
        let target = point(8.0, 9.0);
        state.mark_initialized();
        state.begin_settling(target, default_shape(), &tracked, 100.0);

        let should_promote = state.should_promote_settled_target(124.0, target, &tracked);

        assert!(!should_promote);
    }

    #[test]
    fn should_promote_settled_target_returns_true_at_deadline_for_matching_observation() {
        let mut state = RuntimeState::default();
        state.config.delay_event_to_smear = 25.0;
        let tracked = location(1, 2, 3, 4);
        let target = point(8.0, 9.0);
        state.mark_initialized();
        state.begin_settling(target, default_shape(), &tracked, 100.0);

        let should_promote = state.should_promote_settled_target(125.0, target, &tracked);

        assert!(should_promote);
    }

    #[test]
    fn should_promote_settled_target_requires_matching_observation_surface() {
        let mut state = RuntimeState::default();
        state.config.delay_event_to_smear = 25.0;
        let tracked = location(1, 2, 3, 4);
        let target = point(8.0, 9.0);
        state.mark_initialized();
        state.begin_settling(target, default_shape(), &tracked, 100.0);

        let should_promote =
            state.should_promote_settled_target(130.0, target, &location(1, 2, 5, 6));

        assert!(!should_promote);
    }

    #[test]
    fn note_settle_probe_requires_hold_threshold_consecutive_enter_frames() {
        let mut state = RuntimeState::default();
        state.config.stop_hold_frames = 3;
        state.start_animation();

        let first = state.note_settle_probe(true);
        let second = state.note_settle_probe(true);
        let third = state.note_settle_probe(true);

        assert!(!first);
        assert!(!second);
        assert!(third);
    }

    #[test]
    fn note_settle_probe_resets_hold_counter_after_non_enter_frame() {
        let mut state = RuntimeState::default();
        state.config.stop_hold_frames = 3;
        state.start_animation();
        state.note_settle_probe(true);
        state.note_settle_probe(true);
        state.note_settle_probe(false);

        let promoted_after_reset = state.note_settle_probe(true);

        assert!(!promoted_after_reset);
    }

    #[test]
    fn clear_pending_target_exits_the_settling_phase() {
        let mut state = RuntimeState::default();
        let tracked = location(1, 2, 3, 4);
        state.mark_initialized();
        state.begin_settling(point(8.0, 9.0), default_shape(), &tracked, 100.0);

        state.clear_pending_target();

        assert!(!state.is_settling());
        assert!(state.pending_target().is_none());
    }
}

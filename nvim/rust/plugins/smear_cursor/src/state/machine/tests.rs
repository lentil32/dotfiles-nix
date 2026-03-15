use super::{CursorLocation, CursorShape, RuntimeState};
use crate::animation::{center, corners_for_cursor, initial_velocity};
use crate::types::{Particle, Point, StepOutput};

#[test]
fn animation_phase_transitions_preserve_invariants() {
    let mut state = RuntimeState::default();
    assert!(state.is_enabled());
    assert!(!state.is_initialized());
    assert!(!state.is_animating());

    state.mark_initialized();
    assert!(state.is_initialized());
    assert!(!state.is_animating());

    state.start_animation();
    assert!(state.is_initialized());
    assert!(state.is_animating());

    state.stop_animation();
    assert!(state.is_initialized());
    assert!(!state.is_animating());

    state.clear_initialization();
    assert!(!state.is_initialized());
    assert!(!state.is_animating());
}

#[test]
fn transient_reset_clears_only_transient_fields() {
    let mut state = RuntimeState::default();
    state.set_target(Point { row: 4.0, col: 9.0 }, CursorShape::new(false, false));
    state.update_tracking(&CursorLocation::new(11, 22, 33, 44));
    state.set_color_at_cursor(Some("#ffffff".to_string()));
    state.set_last_tick_ms(Some(99.0));

    state.reset_transient_state();

    assert_eq!(state.target_position(), Point::ZERO);
    assert_eq!(state.tracked_location(), None);
    assert_eq!(state.color_at_cursor(), None);
    assert_eq!(state.last_tick_ms(), None);
}

#[test]
fn retarget_epoch_advances_only_on_target_position_changes() {
    let mut state = RuntimeState::default();
    let location = CursorLocation::new(1, 2, 3, 4);
    let origin = Point { row: 5.0, col: 6.0 };
    assert_eq!(state.retarget_epoch(), 0);

    state.initialize_cursor(origin, CursorShape::new(false, false), 7, &location);
    let after_initialize = state.retarget_epoch();
    assert!(after_initialize > 0);

    state.set_target(origin, CursorShape::new(false, false));
    assert_eq!(state.retarget_epoch(), after_initialize);

    let moved = Point { row: 5.0, col: 9.0 };
    state.set_target(moved, CursorShape::new(false, false));
    assert_eq!(state.retarget_epoch(), after_initialize.wrapping_add(1));

    state.jump_and_stop_animation(moved, CursorShape::new(false, false), &location);
    assert_eq!(state.retarget_epoch(), after_initialize.wrapping_add(1));
}

#[test]
fn retarget_epoch_advances_when_tracking_surface_changes() {
    let mut state = RuntimeState::default();
    let initial_location = CursorLocation::new(1, 2, 3, 4);
    state.initialize_cursor(
        Point { row: 5.0, col: 6.0 },
        CursorShape::new(false, false),
        7,
        &initial_location,
    );
    let baseline_epoch = state.retarget_epoch();

    state.update_tracking(&initial_location);
    assert_eq!(state.retarget_epoch(), baseline_epoch);

    state.update_tracking(&CursorLocation::new(9, 2, 3, 4));
    assert_eq!(state.retarget_epoch(), baseline_epoch.wrapping_add(1));

    state.update_tracking(&CursorLocation::new(9, 88, 3, 4));
    assert_eq!(state.retarget_epoch(), baseline_epoch.wrapping_add(2));
}

#[test]
fn trail_stroke_id_advances_on_jump_like_transitions_but_not_plain_retargets() {
    let mut state = RuntimeState::default();
    let location = CursorLocation::new(1, 2, 3, 4);
    let origin = Point { row: 5.0, col: 6.0 };
    let shape = CursorShape::new(false, false);

    assert_eq!(state.trail_stroke_id().value(), 0);

    state.initialize_cursor(origin, shape, 7, &location);
    let after_initialize = state.trail_stroke_id();
    assert!(after_initialize.value() > 0);

    state.set_target(Point { row: 5.0, col: 9.0 }, shape);
    assert_eq!(state.trail_stroke_id(), after_initialize);

    state.jump_preserving_motion(Point { row: 8.0, col: 9.0 }, shape, &location);
    let after_preserving_jump = state.trail_stroke_id();
    assert!(after_preserving_jump > after_initialize);

    state.jump_and_stop_animation(Point { row: 9.0, col: 9.0 }, shape, &location);
    assert!(state.trail_stroke_id() > after_preserving_jump);
}

#[test]
fn initialize_cursor_resets_motion_and_tracks_cursor() {
    let mut state = RuntimeState::default();
    state.start_animation();
    state.velocity_corners = [Point { row: 1.0, col: 2.0 }; 4];
    state.particles.push(crate::types::Particle {
        position: Point { row: 0.0, col: 0.0 },
        velocity: Point { row: 0.0, col: 0.0 },
        lifetime: 1.0,
    });

    let location = CursorLocation::new(10, 20, 30, 40);
    let shape = CursorShape::new(false, false);
    let position = Point { row: 8.0, col: 9.0 };
    state.initialize_cursor(position, shape, 123, &location);

    let expected_corners = corners_for_cursor(position.row, position.col, false, false);
    assert!(state.is_initialized());
    assert!(!state.is_animating());
    assert_eq!(state.current_corners, expected_corners);
    assert_eq!(state.target_corners, expected_corners);
    assert_eq!(state.target_position(), position);
    assert_eq!(state.velocity_corners, [Point::ZERO; 4]);
    assert!(state.particles.is_empty());
    assert_eq!(state.rng_state, 123);
    assert_eq!(state.tracked_location(), Some(location));
}

#[test]
fn jump_preserving_motion_keeps_animation_running() {
    let mut state = RuntimeState::default();
    state.mark_initialized();
    state.start_animation();
    state.velocity_corners = [Point { row: 2.0, col: 3.0 }; 4];

    state.jump_preserving_motion(
        Point { row: 4.0, col: 5.0 },
        CursorShape::new(false, false),
        &CursorLocation::new(1, 2, 3, 4),
    );

    assert!(state.is_animating());
    assert_eq!(state.velocity_corners, [Point { row: 2.0, col: 3.0 }; 4]);
}

#[test]
fn sync_to_current_cursor_tracks_current_geometry() {
    let mut state = RuntimeState::default();
    let target = Point { row: 6.0, col: 7.0 };
    let location = CursorLocation::new(1, 2, 3, 4);
    state.sync_to_current_cursor(target, CursorShape::new(false, false), &location);

    assert_eq!(state.target_position(), target);
    assert_eq!(state.tracked_location(), Some(location));
}

#[test]
fn apply_scroll_shift_clamps_cursor_row_and_shifts_particles() {
    let mut state = RuntimeState::default();
    state.initialize_cursor(
        Point { row: 5.0, col: 9.0 },
        CursorShape::new(false, false),
        11,
        &CursorLocation::new(1, 2, 3, 4),
    );
    state.particles.push(Particle {
        position: Point {
            row: 10.0,
            col: 2.0,
        },
        velocity: Point { row: 0.0, col: 0.0 },
        lifetime: 1.0,
    });

    state.apply_scroll_shift(10.0, 1.0, 99.0, false, false);

    let expected_corners = corners_for_cursor(1.0, 9.0, false, false);
    assert_eq!(state.current_corners(), expected_corners);
    assert_eq!(state.previous_center(), center(&expected_corners));
    assert_eq!(state.particles()[0].position.row, 0.0);
}

#[test]
fn start_animation_towards_target_initializes_velocity_from_target_delta() {
    let mut state = RuntimeState::default();
    state.config.anticipation = 0.42;
    state.initialize_cursor(
        Point { row: 3.0, col: 4.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(1, 2, 3, 4),
    );
    state.set_target(Point { row: 8.0, col: 9.0 }, CursorShape::new(false, false));

    let expected_velocity = initial_velocity(
        &state.current_corners(),
        &state.target_corners(),
        state.config.anticipation,
    );
    state.start_animation_towards_target();

    assert!(state.is_animating());
    assert_eq!(state.velocity_corners(), expected_velocity);
}

#[test]
fn apply_step_output_replaces_simulation_fields() {
    let mut state = RuntimeState::default();
    let output = StepOutput {
        current_corners: [Point { row: 4.0, col: 5.0 }; 4],
        velocity_corners: [Point { row: 1.0, col: 2.0 }; 4],
        spring_velocity_corners: [Point {
            row: 0.25,
            col: 0.5,
        }; 4],
        trail_elapsed_ms: [8.0, 8.0, 8.0, 8.0],
        particles: vec![Particle {
            position: Point { row: 6.0, col: 7.0 },
            velocity: Point {
                row: 0.5,
                col: 0.25,
            },
            lifetime: 0.75,
        }],
        previous_center: Point { row: 8.0, col: 9.0 },
        index_head: 0,
        index_tail: 3,
        rng_state: 1234,
    };

    state.apply_step_output(output);

    assert_eq!(state.current_corners(), [Point { row: 4.0, col: 5.0 }; 4]);
    assert_eq!(state.velocity_corners(), [Point { row: 1.0, col: 2.0 }; 4]);
    assert_eq!(
        state.spring_velocity_corners(),
        [Point {
            row: 0.25,
            col: 0.5,
        }; 4]
    );
    assert_eq!(state.trail_elapsed_ms(), [8.0, 8.0, 8.0, 8.0]);
    assert_eq!(state.previous_center(), Point { row: 8.0, col: 9.0 });
    assert_eq!(state.rng_state(), 1234);
    assert_eq!(state.particles().len(), 1);
}

#[test]
fn settling_requires_deadline_and_matching_observation() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 25.0;
    let shape = CursorShape::new(false, false);
    let location = CursorLocation::new(1, 2, 3, 4);
    let target = Point { row: 8.0, col: 9.0 };

    state.mark_initialized();
    state.begin_settling(target, shape, &location, 100.0);
    assert!(state.is_settling());
    assert!(!state.should_promote_settled_target(124.0, target, &location));
    assert!(state.should_promote_settled_target(125.0, target, &location));

    let mismatched_location = CursorLocation::new(1, 2, 5, 6);
    assert!(!state.should_promote_settled_target(130.0, target, &mismatched_location));
}

#[test]
fn settle_probe_respects_hold_frame_threshold() {
    let mut state = RuntimeState::default();
    state.config.stop_hold_frames = 3;

    assert!(!state.note_settle_probe(true));
    assert!(!state.note_settle_probe(true));
    assert!(state.note_settle_probe(true));
    assert!(!state.note_settle_probe(false));
    assert!(!state.note_settle_probe(true));
}

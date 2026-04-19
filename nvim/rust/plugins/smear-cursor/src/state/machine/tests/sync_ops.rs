use super::*;
use proptest::collection::vec;

proptest! {
    #![proptest_config(stateful_config())]

    #[test]
    fn prop_transition_operations_preserve_velocity_and_tracking_contracts(
        initial_position in finite_point(),
        initial_shape in cursor_shape_strategy(),
        initial_location in cursor_location_strategy(),
        transition_position in finite_point(),
        transition_shape in cursor_shape_strategy(),
        transition_location in cursor_location_strategy(),
        transition in cursor_transition_case_strategy(),
        setup_phase in transition_setup_phase_strategy(),
        velocity in finite_point(),
        spring_velocity in finite_point(),
        trail_elapsed in 0.5_f64..32.0_f64,
        particles in vec((finite_point(), finite_point(), 0.1_f64..1.0_f64), 0..4),
        last_tick_ms in prop::option::of(0.0_f64..256.0_f64),
        baseline_rng_state in any::<u32>(),
        pending_position in finite_point(),
        pending_shape in cursor_shape_strategy(),
        pending_location in cursor_location_strategy(),
        pending_now_ms in 0.0_f64..256.0_f64,
    ) {
        let mut state = RuntimeState::default();
        state.initialize_cursor(
            initial_position,
            initial_shape,
            baseline_rng_state,
            &initial_location,
        );
        state.velocity_corners = [velocity; 4];
        state.spring_velocity_corners = [spring_velocity; 4];
        state.trail.elapsed_ms = [trail_elapsed; 4];
        state.particles = particles
            .iter()
            .map(|(position, velocity, lifetime)| Particle {
                position: *position,
                velocity: *velocity,
                lifetime: *lifetime,
            })
            .collect();
        match setup_phase {
            TransitionSetupPhase::Idle => {}
            TransitionSetupPhase::Running => {
                state.start_animation();
                state.set_last_tick_ms(last_tick_ms);
            }
            TransitionSetupPhase::Settling => {
                state.begin_settling(
                    pending_position,
                    pending_shape,
                    &pending_location,
                    pending_now_ms,
                );
            }
        }

        let expected_epoch = state.retarget_epoch().wrapping_add(
            u64::from(state.target_position() != transition_position)
                + u64::from(surface_changed(
                    state.tracked_location_ref(),
                    &transition_location,
                )),
        );
        let baseline_stroke = state.trail_stroke_id();
        let baseline_particles = state.particles().to_vec();
        let baseline_velocity = state.velocity_corners();
        let baseline_rng_state = state.rng_state();
        let baseline_last_tick = state.last_tick_ms();
        let was_animating = state.is_animating();

        match transition {
            CursorTransitionCase::Initialize { seed } => {
                state.initialize_cursor(
                    transition_position,
                    transition_shape,
                    seed,
                    &transition_location,
                );
                prop_assert_eq!(state.velocity_corners(), zero_velocity_corners());
                prop_assert_eq!(state.spring_velocity_corners(), zero_velocity_corners());
                prop_assert!(state.particles().is_empty());
                prop_assert_eq!(state.rng_state(), seed);
                prop_assert_eq!(state.last_tick_ms(), None);
                prop_assert!(state.is_initialized());
                prop_assert!(!state.is_animating());
            }
            CursorTransitionCase::JumpPreservingMotion => {
                state.jump_preserving_motion(
                    transition_position,
                    transition_shape,
                    &transition_location,
                );
                prop_assert_eq!(state.velocity_corners(), baseline_velocity);
                prop_assert_eq!(state.spring_velocity_corners(), zero_velocity_corners());
                prop_assert_eq!(state.particles(), baseline_particles.as_slice());
                prop_assert_eq!(state.rng_state(), baseline_rng_state);
                prop_assert_eq!(state.last_tick_ms(), baseline_last_tick);
                prop_assert!(state.is_initialized());
                prop_assert_eq!(state.is_animating(), was_animating);
            }
            CursorTransitionCase::JumpAndStopAnimation => {
                state.jump_and_stop_animation(
                    transition_position,
                    transition_shape,
                    &transition_location,
                );
                prop_assert_eq!(state.velocity_corners(), zero_velocity_corners());
                prop_assert_eq!(state.spring_velocity_corners(), zero_velocity_corners());
                prop_assert_eq!(state.particles(), baseline_particles.as_slice());
                prop_assert_eq!(state.rng_state(), baseline_rng_state);
                prop_assert_eq!(state.last_tick_ms(), None);
                prop_assert!(state.is_initialized());
                prop_assert!(!state.is_animating());
            }
            CursorTransitionCase::SyncToCurrentCursor => {
                state.sync_to_current_cursor(
                    transition_position,
                    transition_shape,
                    &transition_location,
                );
                prop_assert_eq!(state.velocity_corners(), zero_velocity_corners());
                prop_assert_eq!(state.spring_velocity_corners(), zero_velocity_corners());
                prop_assert!(state.particles().is_empty());
                prop_assert_eq!(state.rng_state(), baseline_rng_state);
                prop_assert_eq!(state.last_tick_ms(), None);
                prop_assert!(state.is_initialized());
                prop_assert!(!state.is_animating());
            }
        }

        let expected_corners = transition_shape.corners(transition_position);
        prop_assert_eq!(state.current_corners(), expected_corners);
        prop_assert_eq!(state.trail_origin_corners(), expected_corners);
        prop_assert_eq!(state.target_corners(), expected_corners);
        prop_assert_eq!(state.target_position(), transition_position);
        prop_assert_eq!(state.previous_center(), center(&expected_corners));
        prop_assert_eq!(state.trail_elapsed_ms(), [0.0; 4]);
        prop_assert_eq!(state.trail_stroke_id(), baseline_stroke.next());
        prop_assert_eq!(state.retarget_epoch(), expected_epoch);
        prop_assert_eq!(state.tracked_location_ref(), Some(&transition_location));
        prop_assert!(state.settling_window().is_none());
    }
}

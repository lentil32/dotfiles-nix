use super::*;
use pretty_assertions::assert_eq;
use proptest::collection::vec;

fn setup_phase(
    state: &mut RuntimeState,
    phase: TransitionSetupPhase,
    pending_position: Point,
    pending_shape: CursorShape,
    pending_location: &CursorLocation,
    now_ms: f64,
) {
    state.initialize_cursor(point(3.0, 4.0), default_shape(), 7, pending_location);
    match phase {
        TransitionSetupPhase::Idle => {}
        TransitionSetupPhase::Running => state.start_animation(),
        TransitionSetupPhase::Settling => {
            state.begin_settling(pending_position, pending_shape, pending_location, now_ms);
        }
    }
}

proptest! {
    #![proptest_config(stateful_config())]

    #[test]
    fn prop_settled_target_promotion_depends_on_deadline_and_observation_match(
        target in finite_point(),
        shape in cursor_shape_strategy(),
        tracked in cursor_location_strategy(),
        delay_event_to_smear in -32.0_f64..64.0_f64,
        start_ms in 0.0_f64..256.0_f64,
        elapsed_ms in -8.0_f64..96.0_f64,
        matching_position in any::<bool>(),
        matching_location in any::<bool>(),
    ) {
        let mut state = RuntimeState::default();
        state.config.delay_event_to_smear = delay_event_to_smear;
        state.mark_initialized();
        state.begin_settling(target, shape, &tracked, start_ms);

        let observed_position = if matching_position {
            target
        } else {
            point(target.row + 1.0, target.col - 1.0)
        };
        let observed_location = if matching_location {
            tracked.clone()
        } else {
            perturbed_location(&tracked)
        };
        let expected_deadline = start_ms + delay_event_to_smear.max(0.0);
        let should_promote = state.should_promote_settled_target(
            start_ms + elapsed_ms,
            observed_position,
            &observed_location,
        );
        let expected_window = SettlingWindow {
            stable_since_ms: start_ms,
            settle_deadline_ms: expected_deadline,
        };

        prop_assert!(state.is_settling());
        prop_assert_eq!(state.target_position(), target);
        prop_assert_eq!(state.tracked_location_ref(), Some(&tracked));
        prop_assert_eq!(state.settling_window().copied(), Some(expected_window));
        prop_assert!(expected_window.settle_deadline_ms >= expected_window.stable_since_ms);
        prop_assert_eq!(
            should_promote,
            matching_position && matching_location && start_ms + elapsed_ms >= expected_deadline
        );
    }

    #[test]
    fn prop_note_settle_probe_matches_consecutive_enter_hold_counter(
        stop_hold_frames in 1_u32..8_u32,
        settle_probes in vec(any::<bool>(), 1..24),
    ) {
        let mut state = RuntimeState::default();
        state.config.stop_hold_frames = stop_hold_frames;
        state.start_animation();
        let mut consecutive_enter_frames = 0_u32;

        for within_enter_threshold in settle_probes {
            consecutive_enter_frames = if within_enter_threshold {
                consecutive_enter_frames.saturating_add(1)
            } else {
                0
            };
            prop_assert_eq!(
                state.note_settle_probe(within_enter_threshold),
                consecutive_enter_frames >= stop_hold_frames,
                "within_enter_threshold={}, consecutive_enter_frames={}, stop_hold_frames={}",
                within_enter_threshold,
                consecutive_enter_frames,
                stop_hold_frames,
            );
        }
    }

    #[test]
    fn prop_clear_pending_target_only_exits_settling_phase(
        phase in transition_setup_phase_strategy(),
        pending_position in finite_point(),
        pending_shape in cursor_shape_strategy(),
        pending_location in cursor_location_strategy(),
        now_ms in 0.0_f64..256.0_f64,
    ) {
        let mut state = RuntimeState::default();
        setup_phase(
            &mut state,
            phase,
            pending_position,
            pending_shape,
            &pending_location,
            now_ms,
        );
        let baseline = state.clone();

        state.clear_pending_target();

        match phase {
            TransitionSetupPhase::Settling => {
                prop_assert!(state.is_initialized());
                prop_assert!(!state.is_settling());
                prop_assert!(state.settling_window().is_none());
                prop_assert_eq!(state.target_position(), baseline.target_position());
                prop_assert_eq!(state.target_corners(), baseline.target_corners());
                prop_assert_eq!(state.tracked_location_ref(), baseline.tracked_location_ref());
            }
            TransitionSetupPhase::Idle | TransitionSetupPhase::Running => {
                prop_assert_eq!(state, baseline);
            }
        }
    }
}

#[test]
fn settling_window_uses_runtime_owned_target_and_tracking_for_promotion() {
    let original_location = location(1, 2, 3, 4);
    let retargeted_location = location(5, 6, 7, 8);
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 20.0;
    state.mark_initialized();
    state.begin_settling(point(8.0, 9.0), default_shape(), &original_location, 100.0);

    assert_eq!(
        state.settling_window().copied(),
        Some(SettlingWindow {
            stable_since_ms: 100.0,
            settle_deadline_ms: 120.0,
        })
    );

    state.set_target(point(12.0, 13.0), default_shape());
    state.update_tracking(&retargeted_location);

    assert!(!state.should_promote_settled_target(120.0, point(8.0, 9.0), &original_location,));
    assert!(state.should_promote_settled_target(120.0, point(12.0, 13.0), &retargeted_location,));
}

#[test]
fn refresh_settling_target_preserves_stable_since_until_runtime_owner_changes() {
    let original_location = location(1, 2, 3, 4);
    let retargeted_location = location(5, 6, 7, 8);
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 20.0;
    state.mark_initialized();
    state.begin_settling(point(8.0, 9.0), default_shape(), &original_location, 100.0);

    state.refresh_settling_target(point(8.0, 9.0), default_shape(), &original_location, 110.0);

    assert_eq!(state.target_position(), point(8.0, 9.0));
    assert_eq!(state.tracked_location_ref(), Some(&original_location));
    assert_eq!(
        state.settling_window().copied(),
        Some(SettlingWindow {
            stable_since_ms: 100.0,
            settle_deadline_ms: 130.0,
        })
    );

    state.refresh_settling_target(
        point(12.0, 13.0),
        default_shape(),
        &retargeted_location,
        115.0,
    );

    assert_eq!(state.target_position(), point(12.0, 13.0));
    assert_eq!(state.tracked_location_ref(), Some(&retargeted_location));
    assert_eq!(
        state.settling_window().copied(),
        Some(SettlingWindow {
            stable_since_ms: 115.0,
            settle_deadline_ms: 135.0,
        })
    );
}

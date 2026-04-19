use super::*;
use crate::animation::scaled_corners_for_trail;
use pretty_assertions::assert_eq;

fn same_window_large_jump() -> (RuntimeState, CursorTransition) {
    let (mut state, _) = initialized_runtime("n", |state| {
        state.config.delay_event_to_smear = 0.0;
    });
    let transition = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 30.0, 120.0),
        EventSource::External,
    );
    (state, transition)
}

fn cross_window_large_jump() -> (RuntimeState, CursorTransition) {
    let (mut state, _) = initialized_runtime("n", |state| {
        state.config.delay_event_to_smear = 0.0;
    });
    let transition = reduce_cursor_event(
        &mut state,
        "n",
        event_with_location(5.0, 46.0, 120.0, 99, 999, 20),
        EventSource::External,
    );
    (state, transition)
}

#[test]
fn cross_window_large_moves_draw_with_reuse_only_allocation_and_keep_animating() {
    let (state, transition) = cross_window_large_jump();
    let frame = draw_frame(&transition).expect("discontinuous jumps should still draw");

    assert_eq!(transition.motion_class, MotionClass::DiscontinuousJump);
    assert!(transition.should_schedule_next_animation);
    assert!(state.is_animating());
    assert_eq!(
        render_allocation_policy(&transition),
        RenderAllocationPolicy::ReuseOnly
    );
    assert_eq!(frame.trail_stroke_id, state.trail_stroke_id());
}

#[test]
fn same_window_and_cross_window_large_jumps_share_the_same_discontinuous_draw_path() {
    let (same_window, same_window_transition) = same_window_large_jump();
    let (cross_window, cross_window_transition) = cross_window_large_jump();

    assert_eq!(
        same_window_transition.motion_class,
        MotionClass::DiscontinuousJump
    );
    assert_eq!(
        cross_window_transition.motion_class,
        MotionClass::DiscontinuousJump
    );
    assert!(same_window_transition.should_schedule_next_animation);
    assert!(cross_window_transition.should_schedule_next_animation);
    assert!(same_window.is_animating());
    assert!(cross_window.is_animating());
    assert_eq!(
        render_allocation_policy(&same_window_transition),
        RenderAllocationPolicy::ReuseOnly
    );
    assert_eq!(
        render_allocation_policy(&cross_window_transition),
        RenderAllocationPolicy::ReuseOnly
    );
}

#[test]
fn jump_class_retargets_clear_the_current_plan_when_continuous_smear_is_not_allowed() {
    let (mut state, _) = animating_runtime_after_kickoff(|state| {
        state.config.delay_event_to_smear = 0.0;
    });
    state.config.smear_horizontally = false;

    let retarget = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 24.0, 120.0),
        EventSource::External,
    );

    assert!(matches!(render_action(&retarget), RenderAction::ClearAll));
    assert_eq!(
        render_cleanup_action(&retarget),
        RenderCleanupAction::Schedule
    );
}

#[test]
fn jump_class_retargets_advance_the_trail_stroke_and_stop_animation() {
    let (mut state, kickoff) = animating_runtime_after_kickoff(|state| {
        state.config.delay_event_to_smear = 0.0;
    });
    let kickoff_stroke_id = draw_frame(&kickoff)
        .map(|frame| frame.trail_stroke_id)
        .expect("kickoff should draw");
    state.config.smear_horizontally = false;

    let _ = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 24.0, 120.0),
        EventSource::External,
    );
    assert!(state.trail_stroke_id() > kickoff_stroke_id);
    assert!(!state.is_animating());
}

#[test]
fn window_hop_motion_no_longer_requires_midpoint_coverage() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.smear_between_windows = true;
    state.config.smear_between_buffers = true;

    let _ = reduce_cursor_event(&mut state, "n", event(5.0, 6.0), EventSource::External);
    let _ = reduce_cursor_event(
        &mut state,
        "n",
        event_with_location(5.0, 46.0, 120.0, 99, 999, 20),
        EventSource::External,
    );

    let midpoint_col = 26.0_f64;
    let mut now_ms = 128.0_f64;
    let mut crossed_midpoint = false;
    for _ in 0..24 {
        let effects = reduce_cursor_event(
            &mut state,
            "n",
            event_with_location(5.0, 46.0, now_ms, 100, 999, 20),
            EventSource::AnimationTick,
        );
        if let Some(frame) = draw_frame(&effects) {
            let min_col = frame
                .corners
                .iter()
                .map(|corner| corner.col)
                .fold(f64::INFINITY, f64::min);
            let max_col = frame
                .corners
                .iter()
                .map(|corner| corner.col)
                .fold(f64::NEG_INFINITY, f64::max);
            if min_col <= midpoint_col && midpoint_col <= max_col {
                crossed_midpoint = true;
                break;
            }
        }
        now_ms += 16.0;
    }

    // Phase 1 motion uses a center-based second-order filter, so this path no longer relies on
    // wide corner-ranked geometry spans to keep midpoint coverage during large window hops.
    assert!(!crossed_midpoint);
}

#[test]
fn replace_mode_horizontal_bar_retargets_stay_continuous_for_same_row_motion() {
    let (mut state, _) = initialized_runtime("R", |state| {
        state.config.delay_event_to_smear = 0.0;
        state.config.smear_diagonally = false;
    });

    let retarget = reduce_cursor_event(
        &mut state,
        "R",
        event_at(5.0, 10.0, 120.0),
        EventSource::External,
    );

    assert_eq!(retarget.motion_class, MotionClass::Continuous);
    assert!(matches!(render_action(&retarget), RenderAction::Draw(_)));
    assert!(state.is_animating());
    assert_eq!(
        state.target_position(),
        RenderPoint {
            row: 5.0,
            col: 10.0
        }
    );
}

#[test]
fn scaled_head_retargets_use_the_live_visual_anchor() {
    let (mut state, _) = initialized_runtime("n", |state| {
        state.config.delay_event_to_smear = 0.0;
        state.config.smear_diagonally = false;
        state.config.trail_thickness = 3.0;
        state.config.trail_thickness_x = 1.6;
    });
    let scaled_corners = scaled_corners_for_trail(
        &state.current_corners(),
        state.config.trail_thickness,
        state.config.trail_thickness_x,
    );
    state.apply_step_output(StepOutput {
        current_corners: scaled_corners,
        velocity_corners: state.velocity_corners(),
        spring_velocity_corners: state.spring_velocity_corners(),
        trail_elapsed_ms: state.trail_elapsed_ms(),
        particles: state.particles().to_vec(),
        previous_center: corners_center(&scaled_corners),
        index_head: 0,
        index_tail: 0,
        rng_state: state.rng_state(),
    });

    let retarget = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 10.0, 120.0),
        EventSource::External,
    );

    assert_eq!(retarget.motion_class, MotionClass::Continuous);
    assert!(matches!(render_action(&retarget), RenderAction::Draw(_)));
    assert!(state.is_animating());
    assert_eq!(
        state.target_position(),
        RenderPoint {
            row: 5.0,
            col: 10.0
        }
    );
}

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn long_gap_while_animating_clears_stale_motion_and_stops_animation() {
    let mut state = animating_runtime_towards_target(|state| {
        state.config.time_interval = 16.0;
        state.config.simulation_hz = 120.0;
        state.config.max_simulation_steps_per_frame = 16;
    });
    let expected_corners = state.target_corners();
    let baseline_stroke = state.trail_stroke_id();

    let transition = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 12.0, 2_500.0),
        EventSource::AnimationTick,
    );

    assert_eq!(
        TransitionSummary::from_transition(&transition),
        TransitionSummary {
            motion_class: MotionClass::DiscontinuousJump,
            animation_schedule: crate::core::types::AnimationSchedule::Idle,
            render_cleanup_action: RenderCleanupAction::Schedule,
            render_allocation_policy: RenderAllocationPolicy::ReuseOnly,
            render_side_effects: RenderSideEffects {
                redraw_after_draw_if_cmdline: false,
                redraw_after_clear_if_cmdline: false,
                target_cell_presentation: TargetCellPresentation::None,
                cursor_visibility: CursorVisibilityEffect::Show,
                allow_real_cursor_updates: true,
            },
            render_action: RenderActionSummary::ClearAll,
        }
    );
    assert_eq!(state.current_corners(), expected_corners);
    assert_eq!(state.trail_origin_corners(), expected_corners);
    assert_eq!(state.trail_elapsed_ms(), [0.0; 4]);
    assert_eq!(state.trail_stroke_id(), baseline_stroke.next());
    assert_eq!(state.last_tick_ms(), None);
    assert!(state.particles().is_empty());
    assert!(!state.is_animating());
    assert!(!state.is_draining());
}

#[test]
fn long_gap_while_draining_clears_the_tail_and_stops_animation() {
    let mut state = animating_runtime_towards_target(|state| {
        state.config.time_interval = 16.0;
        state.config.simulation_hz = 120.0;
        state.config.max_simulation_steps_per_frame = 16;
    });
    let _ = advance_until_tail_drain(&mut state);
    let expected_corners = state.target_corners();
    let baseline_stroke = state.trail_stroke_id();

    let transition = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 12.0, 10_000.0),
        EventSource::AnimationTick,
    );

    assert_eq!(
        TransitionSummary::from_transition(&transition),
        TransitionSummary {
            motion_class: MotionClass::DiscontinuousJump,
            animation_schedule: crate::core::types::AnimationSchedule::Idle,
            render_cleanup_action: RenderCleanupAction::Schedule,
            render_allocation_policy: RenderAllocationPolicy::ReuseOnly,
            render_side_effects: RenderSideEffects {
                redraw_after_draw_if_cmdline: false,
                redraw_after_clear_if_cmdline: false,
                target_cell_presentation: TargetCellPresentation::None,
                cursor_visibility: CursorVisibilityEffect::Show,
                allow_real_cursor_updates: true,
            },
            render_action: RenderActionSummary::ClearAll,
        }
    );
    assert_eq!(state.current_corners(), expected_corners);
    assert_eq!(state.trail_origin_corners(), expected_corners);
    assert_eq!(state.trail_elapsed_ms(), [0.0; 4]);
    assert_eq!(state.trail_stroke_id(), baseline_stroke.next());
    assert_eq!(state.last_tick_ms(), None);
    assert!(state.particles().is_empty());
    assert!(!state.is_animating());
    assert!(!state.is_draining());
}

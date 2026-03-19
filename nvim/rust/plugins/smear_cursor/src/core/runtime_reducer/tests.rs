use super::{
    CleanupDirective, CleanupPolicyInput, CursorEventContext, CursorTransition,
    CursorVisibilityEffect, EventSource, MotionClass, RenderAction, RenderAllocationPolicy,
    RenderCleanupAction, RenderSideEffects, ScrollShift, decide_cleanup_directive,
    keep_warm_until_ms, next_cleanup_check_delay_ms, reduce_cursor_event,
};
use crate::config::RuntimeConfig;
use crate::core::state::SemanticEvent;
use crate::state::{CursorLocation, CursorShape, JumpCuePhase, RuntimeState};
use crate::types::{Particle, Point, RenderFrame, StepOutput};
use proptest::collection::vec;
use proptest::prelude::*;
use std::sync::Arc;

fn render_action(transition: &CursorTransition) -> &RenderAction {
    &transition.render_decision.render_action
}

fn render_cleanup_action(transition: &CursorTransition) -> RenderCleanupAction {
    transition.render_decision.render_cleanup_action
}

fn render_allocation_policy(transition: &CursorTransition) -> RenderAllocationPolicy {
    transition.render_decision.render_allocation_policy
}

fn render_side_effects(transition: &CursorTransition) -> RenderSideEffects {
    transition.render_decision.render_side_effects
}

fn draw_frame(transition: &CursorTransition) -> Option<&RenderFrame> {
    match render_action(transition) {
        RenderAction::Draw(frame) => Some(frame.as_ref()),
        RenderAction::ClearAll | RenderAction::Noop => None,
    }
}

fn event(row: f64, col: f64) -> CursorEventContext {
    event_at(row, col, 100.0)
}

fn event_at(row: f64, col: f64, now_ms: f64) -> CursorEventContext {
    CursorEventContext {
        row,
        col,
        now_ms,
        seed: 7,
        cursor_location: CursorLocation::new(10, 20, 1, 1),
        scroll_shift: None,
        semantic_event: SemanticEvent::FrameCommitted,
    }
}

fn event_with_location(
    row: f64,
    col: f64,
    now_ms: f64,
    seed: u32,
    window_handle: i64,
    buffer_handle: i64,
) -> CursorEventContext {
    CursorEventContext {
        row,
        col,
        now_ms,
        seed,
        cursor_location: CursorLocation::new(window_handle, buffer_handle, 1, 1),
        scroll_shift: None,
        semantic_event: SemanticEvent::FrameCommitted,
    }
}

#[derive(Clone)]
struct TrajectoryStep {
    source: EventSource,
    event: CursorEventContext,
}

fn event_with_location_and_scroll(
    row: f64,
    col: f64,
    now_ms: f64,
    seed: u32,
    window_handle: i64,
    buffer_handle: i64,
    scroll_shift: Option<ScrollShift>,
) -> CursorEventContext {
    CursorEventContext {
        row,
        col,
        now_ms,
        seed,
        cursor_location: CursorLocation::new(window_handle, buffer_handle, 1, 1),
        scroll_shift,
        semantic_event: SemanticEvent::FrameCommitted,
    }
}

fn text_mutation_event(row: f64, col: f64, now_ms: f64) -> CursorEventContext {
    CursorEventContext {
        semantic_event: SemanticEvent::TextMutatedAtCursorContext,
        ..event_at(row, col, now_ms)
    }
}

fn external_step(
    row: f64,
    col: f64,
    now_ms: f64,
    seed: u32,
    window_handle: i64,
    buffer_handle: i64,
) -> TrajectoryStep {
    TrajectoryStep {
        source: EventSource::External,
        event: event_with_location(row, col, now_ms, seed, window_handle, buffer_handle),
    }
}

fn animation_tick_step(
    row: f64,
    col: f64,
    now_ms: f64,
    seed: u32,
    window_handle: i64,
    buffer_handle: i64,
) -> TrajectoryStep {
    TrajectoryStep {
        source: EventSource::AnimationTick,
        event: event_with_location(row, col, now_ms, seed, window_handle, buffer_handle),
    }
}

fn external_with_scroll_step(
    row: f64,
    col: f64,
    now_ms: f64,
    seed: u32,
    window_handle: i64,
    buffer_handle: i64,
    scroll_shift: ScrollShift,
) -> TrajectoryStep {
    TrajectoryStep {
        source: EventSource::External,
        event: event_with_location_and_scroll(
            row,
            col,
            now_ms,
            seed,
            window_handle,
            buffer_handle,
            Some(scroll_shift),
        ),
    }
}

fn animation_tick_with_scroll_step(
    row: f64,
    col: f64,
    now_ms: f64,
    seed: u32,
    window_handle: i64,
    buffer_handle: i64,
    scroll_shift: ScrollShift,
) -> TrajectoryStep {
    TrajectoryStep {
        source: EventSource::AnimationTick,
        event: event_with_location_and_scroll(
            row,
            col,
            now_ms,
            seed,
            window_handle,
            buffer_handle,
            Some(scroll_shift),
        ),
    }
}

fn quantize_milli(value: f64) -> u64 {
    let scaled = (value * 1_000.0).round() as i64;
    u64::from_ne_bytes(scaled.to_ne_bytes())
}

fn mix_fingerprint(hash: u64, value: u64) -> u64 {
    const FNV_PRIME: u64 = 1_099_511_628_211;
    (hash ^ value).wrapping_mul(FNV_PRIME)
}

fn trajectory_center(state: &RuntimeState) -> Point {
    let corners = state.current_corners();
    Point {
        row: (corners[0].row + corners[1].row + corners[2].row + corners[3].row) / 4.0,
        col: (corners[0].col + corners[1].col + corners[2].col + corners[3].col) / 4.0,
    }
}

fn trajectory_fingerprint(state: &mut RuntimeState, mode: &str, steps: &[TrajectoryStep]) -> u64 {
    let mut hash = 1_469_598_103_934_665_603_u64;

    for step in steps {
        let transition = reduce_cursor_event(state, mode, step.event.clone(), step.source);
        let center = trajectory_center(state);
        let target = state.target_position();
        hash = mix_fingerprint(hash, quantize_milli(step.event.now_ms));
        hash = mix_fingerprint(hash, quantize_milli(center.row));
        hash = mix_fingerprint(hash, quantize_milli(center.col));
        hash = mix_fingerprint(hash, quantize_milli(target.row));
        hash = mix_fingerprint(hash, quantize_milli(target.col));
        hash = mix_fingerprint(hash, u64::from(transition.should_schedule_next_animation));
        hash = mix_fingerprint(hash, u64::from(state.is_animating()));
        hash = mix_fingerprint(hash, u64::from(state.is_settling()));
        hash = mix_fingerprint(hash, state.trail_stroke_id().value());
        hash = mix_fingerprint(hash, state.retarget_epoch());
        let action_tag = match render_action(&transition) {
            RenderAction::Draw(_) => 1_u64,
            RenderAction::ClearAll => 2_u64,
            RenderAction::Noop => 3_u64,
        };
        hash = mix_fingerprint(hash, action_tag);
    }

    hash
}

fn trajectory_fingerprint_with_fresh_state(
    state: &RuntimeState,
    mode: &str,
    steps: &[TrajectoryStep],
) -> u64 {
    let mut replay = RuntimeState::default();
    replay.config = state.config.clone();
    trajectory_fingerprint(&mut replay, mode, steps)
}

fn initialized_runtime(
    mode: &str,
    configure: impl FnOnce(&mut RuntimeState),
) -> (RuntimeState, CursorTransition) {
    let mut state = RuntimeState::default();
    configure(&mut state);
    let transition = reduce_cursor_event(&mut state, mode, event(5.0, 6.0), EventSource::External);
    (state, transition)
}

fn delayed_retarget_scenario() -> (RuntimeState, CursorTransition) {
    let (mut state, _) = initialized_runtime("n", |state| {
        state.config.delay_event_to_smear = 40.0;
    });
    let pending = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 16.0, 116.0),
        EventSource::External,
    );
    (state, pending)
}

fn animating_runtime_after_kickoff(
    configure: impl FnOnce(&mut RuntimeState),
) -> (RuntimeState, CursorTransition) {
    let (mut state, _) = initialized_runtime("n", configure);
    let kickoff = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 12.0, 116.0),
        EventSource::External,
    );
    (state, kickoff)
}

fn animating_runtime_towards_target(configure: impl FnOnce(&mut RuntimeState)) -> RuntimeState {
    let mut state = RuntimeState::default();
    configure(&mut state);
    state.initialize_cursor(
        Point { row: 5.0, col: 6.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(10, 20, 1, 1),
    );
    state.set_target(
        Point {
            row: 5.0,
            col: 12.0,
        },
        CursorShape::new(false, false),
    );
    state.start_animation_towards_target();
    state.set_last_tick_ms(Some(100.0));
    state
}

fn advance_until_tail_drain(state: &mut RuntimeState) -> (u32, CursorTransition) {
    // Surprising: stateful springs make settle timing highly trajectory-dependent, so callers
    // should assert lifecycle invariants here rather than exact frame counts.
    for tick in 1_u32..=160_u32 {
        let now_ms = 100.0 + 16.0 * f64::from(tick);
        let transition = reduce_cursor_event(
            state,
            "n",
            event_at(5.0, 12.0, now_ms),
            EventSource::AnimationTick,
        );
        if state.is_draining() {
            return (tick, transition);
        }
    }

    panic!("animation should eventually settle and start draining");
}

fn advance_until_tail_drain_completes(state: &mut RuntimeState) -> (f64, CursorTransition) {
    for tick in 1_u32..=160_u32 {
        let now_ms = 100.0 + 16.0 * f64::from(tick);
        let transition = reduce_cursor_event(
            state,
            "n",
            event_at(5.0, 12.0, now_ms),
            EventSource::AnimationTick,
        );
        if !state.is_draining() && matches!(render_action(&transition), RenderAction::ClearAll) {
            return (now_ms, transition);
        }
    }

    panic!("expected tail drain to finish with an explicit clear-all");
}

mod cleanup_policy {
    use super::*;

    #[test]
    fn keeps_pool_warm_before_soft_threshold() {
        let directive = decide_cleanup_directive(CleanupPolicyInput {
            idle_ms: 200,
            soft_cleanup_delay_ms: 200,
            hard_cleanup_delay_ms: 3_000,
            pool_total_windows: 24,
            recent_frame_demand: 20,
            max_kept_windows: 256,
            callback_duration_estimate_ms: 4.0,
        });
        assert_eq!(directive, CleanupDirective::KeepWarm);
    }

    #[test]
    fn soft_clears_mid_idle() {
        let directive = decide_cleanup_directive(CleanupPolicyInput {
            idle_ms: 800,
            soft_cleanup_delay_ms: 200,
            hard_cleanup_delay_ms: 3_000,
            pool_total_windows: 24,
            recent_frame_demand: 20,
            max_kept_windows: 64,
            callback_duration_estimate_ms: 2.0,
        });
        assert_eq!(
            directive,
            CleanupDirective::SoftClear {
                max_kept_windows: 64
            }
        );
    }

    #[test]
    fn hard_purges_after_long_idle() {
        let directive = decide_cleanup_directive(CleanupPolicyInput {
            idle_ms: 3_100,
            soft_cleanup_delay_ms: 200,
            hard_cleanup_delay_ms: 3_000,
            pool_total_windows: 24,
            recent_frame_demand: 20,
            max_kept_windows: 64,
            callback_duration_estimate_ms: 2.0,
        });
        assert_eq!(directive, CleanupDirective::HardPurge);
    }

    #[test]
    fn noops_when_pool_is_empty() {
        let directive = decide_cleanup_directive(CleanupPolicyInput {
            idle_ms: 10_000,
            soft_cleanup_delay_ms: 200,
            hard_cleanup_delay_ms: 3_000,
            pool_total_windows: 0,
            recent_frame_demand: 0,
            max_kept_windows: 64,
            callback_duration_estimate_ms: 10.0,
        });
        assert_eq!(directive, CleanupDirective::KeepWarm);
    }

    #[test]
    fn keep_warm_rearms_until_penalty_window_elapses() {
        let input = CleanupPolicyInput {
            idle_ms: 200,
            soft_cleanup_delay_ms: 200,
            hard_cleanup_delay_ms: 3_000,
            pool_total_windows: 24,
            recent_frame_demand: 20,
            max_kept_windows: 64,
            callback_duration_estimate_ms: 50.0,
        };

        assert_eq!(decide_cleanup_directive(input), CleanupDirective::KeepWarm);
        assert_eq!(
            next_cleanup_check_delay_ms(input),
            Some(keep_warm_until_ms(input).saturating_sub(input.idle_ms))
        );
    }

    #[test]
    fn soft_clear_rearms_until_hard_purge_horizon() {
        let input = CleanupPolicyInput {
            idle_ms: 800,
            soft_cleanup_delay_ms: 200,
            hard_cleanup_delay_ms: 3_000,
            pool_total_windows: 24,
            recent_frame_demand: 20,
            max_kept_windows: 64,
            callback_duration_estimate_ms: 2.0,
        };

        assert_eq!(
            decide_cleanup_directive(input),
            CleanupDirective::SoftClear {
                max_kept_windows: 64,
            }
        );
        assert_eq!(next_cleanup_check_delay_ms(input), Some(2_200));
    }
}

mod bootstrap_and_frame_building {
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
    fn first_external_event_draws_a_frame() {
        let (_, transition) = initialized_runtime("n", |_| {});
        assert!(matches!(render_action(&transition), RenderAction::Draw(_)));
    }

    #[test]
    fn first_external_event_schedules_cleanup_and_bootstrap_allocation() {
        let (_, transition) = initialized_runtime("n", |_| {});
        assert_eq!(
            render_cleanup_action(&transition),
            RenderCleanupAction::Schedule
        );
        assert_eq!(
            render_allocation_policy(&transition),
            RenderAllocationPolicy::BootstrapIfPoolEmpty
        );
    }

    #[test]
    fn first_external_event_initializes_without_starting_animation() {
        let (state, _) = initialized_runtime("n", |_| {});
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
}

mod cursor_visibility_side_effects {
    use super::*;

    #[test]
    fn draw_effects_show_cursor_when_frame_overlaps_target() {
        let (_, transition) = initialized_runtime("n", |_| {});
        let side_effects = render_side_effects(&transition);
        assert_eq!(side_effects.cursor_visibility, CursorVisibilityEffect::Show);
        assert!(side_effects.allow_real_cursor_updates);
        assert!(!side_effects.redraw_after_draw_if_cmdline);
    }

    #[test]
    fn clear_effects_show_cursor_and_redraw_in_cmdline_mode() {
        let (mut state, _) = initialized_runtime("n", |_| {});
        state.set_enabled(false);

        let effects = reduce_cursor_event(&mut state, "c", event(3.0, 8.0), EventSource::External);
        let side_effects = render_side_effects(&effects);
        assert_eq!(side_effects.cursor_visibility, CursorVisibilityEffect::Show);
        assert!(side_effects.allow_real_cursor_updates);
        assert!(side_effects.redraw_after_clear_if_cmdline);
    }

    #[test]
    fn target_hack_disables_real_cursor_updates_in_emitted_side_effects() {
        let mut state = RuntimeState::default();
        state.config.hide_target_hack = true;
        state.set_enabled(false);

        let effects = reduce_cursor_event(&mut state, "n", event(3.0, 8.0), EventSource::External);
        let side_effects = render_side_effects(&effects);

        assert_eq!(side_effects.cursor_visibility, CursorVisibilityEffect::Show);
        assert!(!side_effects.allow_real_cursor_updates);
    }

    #[test]
    fn draw_effects_skip_redraw_in_cmdline_mode() {
        let mut state = RuntimeState::default();
        state.config.smear_to_cmd = true;
        let effects = reduce_cursor_event(&mut state, "c", event(5.0, 6.0), EventSource::External);
        let side_effects = render_side_effects(&effects);
        assert!(!side_effects.redraw_after_draw_if_cmdline);
    }
}

mod delayed_settling_transitions {
    use super::*;

    #[test]
    fn delayed_external_retarget_emits_noop_while_scheduling_the_settle_deadline() {
        let (state, pending) = delayed_retarget_scenario();
        assert!(matches!(render_action(&pending), RenderAction::Noop));
        assert!(pending.should_schedule_next_animation);
        assert_eq!(
            render_side_effects(&pending).cursor_visibility,
            CursorVisibilityEffect::Show
        );
        assert!(state.is_settling());
        assert!(!state.is_animating());
    }

    #[test]
    fn animation_ticks_before_the_delay_deadline_keep_the_runtime_in_settling() {
        let (mut state, _) = delayed_retarget_scenario();
        let early_tick = reduce_cursor_event(
            &mut state,
            "n",
            event_at(5.0, 16.0, 130.0),
            EventSource::AnimationTick,
        );
        assert!(matches!(render_action(&early_tick), RenderAction::Noop));
        assert!(early_tick.should_schedule_next_animation);
        assert_eq!(
            render_side_effects(&early_tick).cursor_visibility,
            CursorVisibilityEffect::Show
        );
        assert!(state.is_settling());
        assert!(!state.is_animating());
    }

    #[test]
    fn settle_deadline_tick_starts_animation_and_hides_the_real_cursor() {
        let (mut state, _) = delayed_retarget_scenario();
        let ready_tick = reduce_cursor_event(
            &mut state,
            "n",
            event_at(5.0, 16.0, 170.0),
            EventSource::AnimationTick,
        );
        assert!(matches!(render_action(&ready_tick), RenderAction::Draw(_)));
        assert!(ready_tick.should_schedule_next_animation);
        assert!(state.is_animating());
        assert_eq!(
            render_side_effects(&ready_tick).cursor_visibility,
            CursorVisibilityEffect::Hide
        );
    }

    #[test]
    fn returning_to_the_current_cursor_cancels_the_pending_settling_target() {
        let (mut state, _) = delayed_retarget_scenario();
        let returned = reduce_cursor_event(
            &mut state,
            "n",
            event_at(5.0, 6.0, 120.0),
            EventSource::External,
        );
        assert!(matches!(render_action(&returned), RenderAction::Noop));
        assert!(state.pending_target().is_none());
        assert!(!state.is_settling());
    }

    #[test]
    fn returning_to_the_current_cursor_leaves_the_runtime_idle_and_cursor_visible() {
        let (mut state, _) = delayed_retarget_scenario();
        let returned = reduce_cursor_event(
            &mut state,
            "n",
            event_at(5.0, 6.0, 120.0),
            EventSource::External,
        );
        assert!(matches!(render_action(&returned), RenderAction::Noop));
        assert!(!state.is_animating());
        assert_eq!(
            render_side_effects(&returned).cursor_visibility,
            CursorVisibilityEffect::Show
        );
    }
}

mod tail_drain_lifecycle {
    use super::*;

    #[test]
    fn stop_hysteresis_keeps_the_first_enter_frames_in_the_animating_phase() {
        let mut state = animating_runtime_towards_target(|state| {
            state.config.stop_distance_enter = 10.0;
            state.config.stop_distance_exit = 10.0;
            state.config.stop_velocity_enter = 10.0;
            state.config.stop_hold_frames = 3;
        });

        let first = reduce_cursor_event(
            &mut state,
            "n",
            event_at(5.0, 12.0, 116.0),
            EventSource::AnimationTick,
        );
        assert!(matches!(render_action(&first), RenderAction::Draw(_)));
        assert!(state.is_animating());

        let second = reduce_cursor_event(
            &mut state,
            "n",
            event_at(5.0, 12.0, 132.0),
            EventSource::AnimationTick,
        );
        assert!(matches!(render_action(&second), RenderAction::Draw(_)));
        assert!(state.is_animating());
    }

    #[test]
    fn stop_hysteresis_does_not_start_tail_drain_before_the_hold_frame_threshold() {
        let mut state = animating_runtime_towards_target(|state| {
            state.config.stop_distance_enter = 10.0;
            state.config.stop_distance_exit = 10.0;
            state.config.stop_velocity_enter = 10.0;
            state.config.stop_hold_frames = 3;
        });

        let (drained_at_tick, transition) = advance_until_tail_drain(&mut state);
        assert!(
            !matches!(render_action(&transition), RenderAction::ClearAll),
            "settle should drain through renderer frames instead of clear-all"
        );
        assert!(
            drained_at_tick >= state.config.stop_hold_frames,
            "tail drain must not start before stop_hold_frames consecutive enter probes"
        );
    }

    #[test]
    fn entering_tail_drain_draws_the_first_drain_frame_and_preserves_remaining_steps() {
        let mut state = animating_runtime_towards_target(|state| {
            state.config.stop_distance_enter = 10.0;
            state.config.stop_distance_exit = 10.0;
            state.config.stop_velocity_enter = 10.0;
            state.config.stop_hold_frames = 3;
        });

        let (_, transition) = advance_until_tail_drain(&mut state);
        assert!(matches!(render_action(&transition), RenderAction::Draw(_)));
        assert_eq!(
            draw_frame(&transition).map(|frame| frame.planner_idle_steps),
            Some(0)
        );
        assert!(!state.is_animating());
        assert!(state.is_draining());
        assert!(state.drain_steps_remaining() > 0);
    }

    #[test]
    fn tail_drain_frames_advance_planner_only_time_before_cleanup() {
        let mut state = animating_runtime_towards_target(|state| {
            state.config.stop_hold_frames = 1;
        });
        let (drained_at_tick, _) = advance_until_tail_drain(&mut state);
        let mut saw_idle_planner_steps = false;

        for tick in (drained_at_tick + 1)..=160_u32 {
            let now_ms = 100.0 + 16.0 * f64::from(tick);
            let transition = reduce_cursor_event(
                &mut state,
                "n",
                event_at(5.0, 12.0, now_ms),
                EventSource::AnimationTick,
            );
            if let Some(frame) = draw_frame(&transition)
                && frame.planner_idle_steps > 0
            {
                saw_idle_planner_steps = true;
                break;
            }
            if !state.is_draining() {
                break;
            }
        }

        assert!(
            saw_idle_planner_steps,
            "expected drain frames to advance planner-only time"
        );
    }

    #[test]
    fn tail_drain_finishes_with_clear_all_and_cleanup_scheduling() {
        let mut state = animating_runtime_towards_target(|state| {
            state.config.stop_hold_frames = 1;
        });
        let (_, transition) = advance_until_tail_drain_completes(&mut state);
        assert!(matches!(render_action(&transition), RenderAction::ClearAll));
        assert!(!transition.should_schedule_next_animation);
        assert_eq!(
            render_cleanup_action(&transition),
            RenderCleanupAction::Schedule
        );
    }

    #[test]
    fn settled_external_noop_after_tail_drain_keeps_cleanup_scheduled() {
        let mut state = animating_runtime_towards_target(|state| {
            state.config.stop_hold_frames = 1;
        });
        let (settled_at_ms, _) = advance_until_tail_drain_completes(&mut state);
        let follow_up = reduce_cursor_event(
            &mut state,
            "n",
            event_at(5.0, 12.0, settled_at_ms + 8.0),
            EventSource::External,
        );

        assert!(matches!(render_action(&follow_up), RenderAction::Noop));
        assert_eq!(
            render_side_effects(&follow_up).cursor_visibility,
            CursorVisibilityEffect::Show
        );
        assert_eq!(
            render_cleanup_action(&follow_up),
            RenderCleanupAction::Schedule
        );
        assert!(!state.is_animating());
        assert!(!state.is_draining());
    }

    #[test]
    fn quiescent_external_noop_after_first_frame_schedules_cleanup() {
        let (mut state, _) = initialized_runtime("n", |_| {});
        let follow_up = reduce_cursor_event(
            &mut state,
            "n",
            event_at(5.0, 6.0, 116.0),
            EventSource::External,
        );

        assert!(matches!(render_action(&follow_up), RenderAction::Noop));
        assert!(!follow_up.should_schedule_next_animation);
        assert_eq!(
            render_side_effects(&follow_up).cursor_visibility,
            CursorVisibilityEffect::Show
        );
        assert_eq!(
            render_cleanup_action(&follow_up),
            RenderCleanupAction::Schedule
        );
        assert!(!state.is_animating());
        assert!(!state.is_draining());
    }

    #[test]
    fn tail_drain_advances_existing_particles_without_emitting_new_ones() {
        let mut state = RuntimeState::default();
        state.config.particles_enabled = true;
        state.config.particles_per_second = 0.0;
        state.config.particles_per_length = 0.0;
        state.initialize_cursor(
            Point { row: 5.0, col: 6.0 },
            CursorShape::new(false, false),
            7,
            &CursorLocation::new(10, 20, 1, 1),
        );
        state.apply_step_output(StepOutput {
            current_corners: state.current_corners(),
            velocity_corners: state.velocity_corners(),
            spring_velocity_corners: state.spring_velocity_corners(),
            trail_elapsed_ms: state.trail_elapsed_ms(),
            particles: vec![Particle {
                position: Point { row: 5.0, col: 7.0 },
                velocity: Point { row: 2.0, col: 1.0 },
                lifetime: 500.0,
            }],
            previous_center: state.previous_center(),
            index_head: 0,
            index_tail: 0,
            rng_state: state.rng_state(),
        });
        state.start_tail_drain(4);
        state.set_last_tick_ms(Some(100.0));

        let particles_before_drain_tick = state.particles().to_vec();
        let transition = reduce_cursor_event(
            &mut state,
            "n",
            event_at(5.0, 6.0, 116.0),
            EventSource::AnimationTick,
        );
        let frame = draw_frame(&transition).expect("drain tick should draw");
        assert!(frame.planner_idle_steps > 0);

        let particles_after_drain_tick = state.particles().to_vec();
        assert!(
            particles_after_drain_tick.len() <= particles_before_drain_tick.len(),
            "drain ticks must not emit new particles"
        );
        assert!(
            particles_after_drain_tick.len() != particles_before_drain_tick.len()
                || particles_after_drain_tick
                    .iter()
                    .zip(&particles_before_drain_tick)
                    .any(|(after, before)| {
                        after.position != before.position
                            || after.velocity != before.velocity
                            || after.lifetime < before.lifetime
                    }),
            "drain ticks should advance or retire existing particles"
        );
    }
}

mod retargeting_while_animating {
    use super::*;

    #[test]
    fn same_surface_external_retargets_draw_immediately_while_animation_is_running() {
        let (mut state, _) = animating_runtime_after_kickoff(|state| {
            state.config.delay_event_to_smear = 0.0;
        });
        let retarget = reduce_cursor_event(
            &mut state,
            "n",
            event_at(5.0, 24.0, 120.0),
            EventSource::External,
        );
        assert!(matches!(render_action(&retarget), RenderAction::Draw(_)));
        assert!(retarget.should_schedule_next_animation);
        assert!(state.is_animating());
    }

    #[test]
    fn rapid_same_surface_retargets_advance_the_retarget_epoch_and_keep_the_latest_target() {
        let (mut state, kickoff) = animating_runtime_after_kickoff(|state| {
            state.config.delay_event_to_smear = 0.0;
        });
        let kickoff_epoch = draw_frame(&kickoff)
            .map(|frame| frame.retarget_epoch)
            .expect("kickoff should draw");

        let retarget_a = reduce_cursor_event(
            &mut state,
            "n",
            event_at(5.0, 24.0, 120.0),
            EventSource::External,
        );
        let epoch_a = draw_frame(&retarget_a)
            .map(|frame| frame.retarget_epoch)
            .expect("retarget A should draw");

        let retarget_b = reduce_cursor_event(
            &mut state,
            "n",
            event_at(5.0, 36.0, 124.0),
            EventSource::External,
        );
        let epoch_b = draw_frame(&retarget_b)
            .map(|frame| frame.retarget_epoch)
            .expect("retarget B should draw");

        assert!(epoch_a > kickoff_epoch);
        assert!(epoch_b > epoch_a);
        assert_eq!(
            state.target_position(),
            Point {
                row: 5.0,
                col: 36.0
            }
        );
    }

    #[test]
    fn animation_ticks_continue_after_rapid_same_surface_retargets() {
        let (mut state, _) = animating_runtime_after_kickoff(|state| {
            state.config.delay_event_to_smear = 0.0;
        });
        let _ = reduce_cursor_event(
            &mut state,
            "n",
            event_at(5.0, 24.0, 120.0),
            EventSource::External,
        );
        let _ = reduce_cursor_event(
            &mut state,
            "n",
            event_at(5.0, 36.0, 124.0),
            EventSource::External,
        );

        let follow_up_tick = reduce_cursor_event(
            &mut state,
            "n",
            event_at(5.0, 36.0, 132.0),
            EventSource::AnimationTick,
        );
        assert!(matches!(
            render_action(&follow_up_tick),
            RenderAction::Draw(_)
        ));
        assert!(follow_up_tick.should_schedule_next_animation);
        assert_eq!(
            state.target_position(),
            Point {
                row: 5.0,
                col: 36.0
            }
        );
    }

    #[test]
    fn same_surface_retarget_keeps_trail_stroke_id_stable() {
        let (mut state, kickoff) = animating_runtime_after_kickoff(|state| {
            state.config.delay_event_to_smear = 0.0;
        });
        let kickoff_stroke_id = draw_frame(&kickoff)
            .map(|frame| frame.trail_stroke_id)
            .expect("kickoff should draw");

        let retarget = reduce_cursor_event(
            &mut state,
            "n",
            event_at(5.0, 24.0, 120.0),
            EventSource::External,
        );
        let retarget_stroke_id = draw_frame(&retarget)
            .map(|frame| frame.trail_stroke_id)
            .expect("same-surface retarget should draw");

        assert_eq!(retarget_stroke_id, kickoff_stroke_id);
    }
}

mod window_and_buffer_jump_policies {
    use super::*;

    #[test]
    fn buffer_switch_clears_when_inter_buffer_smear_is_disabled() {
        for smear_between_windows in [false, true] {
            let mut state = RuntimeState::default();
            state.config.smear_between_windows = smear_between_windows;
            state.config.smear_between_buffers = false;
            state.config.jump_cues_enabled = false;
            let _ = reduce_cursor_event(&mut state, "n", event(5.0, 6.0), EventSource::External);
            let buffer_switched = reduce_cursor_event(
                &mut state,
                "n",
                event_with_location(20.0, 30.0, 120.0, 99, 10, 999),
                EventSource::External,
            );
            assert!(matches!(
                render_action(&buffer_switched),
                RenderAction::ClearAll
            ));
            assert_eq!(
                render_cleanup_action(&buffer_switched),
                RenderCleanupAction::Schedule
            );
        }
    }

    #[test]
    fn buffer_switch_keeps_animating_when_inter_buffer_smear_is_enabled() {
        for smear_between_windows in [false, true] {
            let mut state = RuntimeState::default();
            state.config.smear_between_windows = smear_between_windows;
            state.config.smear_between_buffers = true;
            state.config.jump_cues_enabled = false;
            let _ = reduce_cursor_event(&mut state, "n", event(5.0, 6.0), EventSource::External);
            let buffer_switched = reduce_cursor_event(
                &mut state,
                "n",
                event_with_location(20.0, 30.0, 120.0, 99, 10, 999),
                EventSource::External,
            );
            assert!(!matches!(
                render_action(&buffer_switched),
                RenderAction::ClearAll
            ));
            assert!(buffer_switched.should_schedule_next_animation);
        }
    }

    #[test]
    fn window_change_draws_when_inter_window_smear_is_enabled() {
        let mut state = RuntimeState::default();
        state.config.delay_event_to_smear = 0.0;
        state.config.smear_between_windows = true;
        state.config.smear_between_buffers = true;

        let _ = reduce_cursor_event(&mut state, "n", event(5.0, 6.0), EventSource::External);
        let transition = reduce_cursor_event(
            &mut state,
            "n",
            event_with_location(5.0, 46.0, 120.0, 99, 999, 20),
            EventSource::External,
        );

        assert!(matches!(render_action(&transition), RenderAction::Draw(_)));
        assert!(transition.should_schedule_next_animation);
        assert!(state.is_animating());
    }

    #[test]
    fn window_change_starts_a_new_trail_stroke_and_classifies_as_a_discontinuous_jump() {
        let mut state = RuntimeState::default();
        state.config.delay_event_to_smear = 0.0;
        state.config.smear_between_windows = true;
        state.config.smear_between_buffers = true;

        let _ = reduce_cursor_event(&mut state, "n", event(5.0, 6.0), EventSource::External);
        let before_stroke_id = state.trail_stroke_id();
        let transition = reduce_cursor_event(
            &mut state,
            "n",
            event_with_location(5.0, 46.0, 120.0, 99, 999, 20),
            EventSource::External,
        );
        let frame = draw_frame(&transition)
            .expect("window hop should draw when inter-window smear is enabled");

        assert!(frame.trail_stroke_id > before_stroke_id);
        assert_eq!(state.trail_stroke_id(), frame.trail_stroke_id);
        assert_eq!(transition.motion_class, MotionClass::DiscontinuousJump);
    }

    #[test]
    fn buffer_change_draws_when_inter_buffer_smear_is_enabled() {
        let mut state = RuntimeState::default();
        state.config.delay_event_to_smear = 0.0;
        state.config.smear_between_windows = true;
        state.config.smear_between_buffers = true;

        let _ = reduce_cursor_event(&mut state, "n", event(5.0, 6.0), EventSource::External);
        let transition = reduce_cursor_event(
            &mut state,
            "n",
            event_with_location(5.0, 46.0, 120.0, 99, 10, 999),
            EventSource::External,
        );

        assert!(matches!(render_action(&transition), RenderAction::Draw(_)));
        assert!(transition.should_schedule_next_animation);
        assert!(state.is_animating());
    }

    #[test]
    fn buffer_change_starts_a_new_trail_stroke_and_classifies_as_a_discontinuous_jump() {
        let mut state = RuntimeState::default();
        state.config.delay_event_to_smear = 0.0;
        state.config.smear_between_windows = true;
        state.config.smear_between_buffers = true;

        let _ = reduce_cursor_event(&mut state, "n", event(5.0, 6.0), EventSource::External);
        let before_stroke_id = state.trail_stroke_id();
        let transition = reduce_cursor_event(
            &mut state,
            "n",
            event_with_location(5.0, 46.0, 120.0, 99, 10, 999),
            EventSource::External,
        );
        let frame = draw_frame(&transition)
            .expect("buffer hop should draw when inter-buffer smear is enabled");

        assert!(frame.trail_stroke_id > before_stroke_id);
        assert_eq!(state.trail_stroke_id(), frame.trail_stroke_id);
        assert_eq!(transition.motion_class, MotionClass::DiscontinuousJump);
    }
}

mod jump_classification_and_cues {
    use super::*;

    fn same_window_large_jump() -> (RuntimeState, CursorTransition) {
        let (mut state, _) = initialized_runtime("n", |state| {
            state.config.delay_event_to_smear = 0.0;
            state.config.jump_cue_min_display_distance = 8.0;
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
            state.config.jump_cue_min_display_distance = 8.0;
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
    fn cross_window_large_moves_draw_with_reuse_only_allocation() {
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
    fn cross_window_large_moves_record_a_launch_cue_from_the_previous_window() {
        let (state, _) = cross_window_large_jump();
        let cues = state.active_jump_cues();

        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].phase, JumpCuePhase::Launch);
        assert_eq!(cues[0].from_pose.window_handle, 10);
        assert_eq!(cues[0].to_pose.window_handle, 999);
    }

    #[test]
    fn same_window_and_cross_window_large_jumps_both_emit_a_single_launch_cue() {
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
        assert_eq!(same_window.active_jump_cues().len(), 1);
        assert_eq!(cross_window.active_jump_cues().len(), 1);
        assert_eq!(
            same_window.active_jump_cues()[0].phase,
            JumpCuePhase::Launch
        );
        assert_eq!(
            cross_window.active_jump_cues()[0].phase,
            JumpCuePhase::Launch
        );
        assert!(same_window_transition.should_schedule_next_animation);
        assert!(cross_window_transition.should_schedule_next_animation);
        assert!(same_window.is_animating());
        assert!(cross_window.is_animating());
    }

    #[test]
    fn same_window_and_cross_window_large_jumps_share_cue_duration_and_strength() {
        let (same_window, _) = same_window_large_jump();
        let (cross_window, _) = cross_window_large_jump();

        assert_eq!(
            same_window.active_jump_cues()[0].duration_ms,
            cross_window.active_jump_cues()[0].duration_ms
        );
        assert_eq!(
            same_window.active_jump_cues()[0].strength,
            cross_window.active_jump_cues()[0].strength
        );
    }

    #[test]
    fn repeated_cross_window_jumps_cap_cue_chain_length() {
        let mut state = RuntimeState::default();
        state.config.delay_event_to_smear = 0.0;
        state.config.jump_cue_min_display_distance = 8.0;
        state.config.jump_cue_max_chain = 2;

        let _ = reduce_cursor_event(&mut state, "n", event(5.0, 6.0), EventSource::External);
        let _ = reduce_cursor_event(
            &mut state,
            "n",
            event_with_location(5.0, 46.0, 120.0, 99, 999, 20),
            EventSource::External,
        );
        let _ = reduce_cursor_event(
            &mut state,
            "n",
            event_with_location(5.0, 6.0, 140.0, 100, 10, 20),
            EventSource::External,
        );
        let _ = reduce_cursor_event(
            &mut state,
            "n",
            event_with_location(5.0, 46.0, 160.0, 101, 777, 20),
            EventSource::External,
        );

        let cues = state.active_jump_cues();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].cue_id, 2);
        assert_eq!(cues[1].cue_id, 3);
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
}

mod render_frame_caching {
    use super::*;

    #[test]
    fn draw_frames_reuse_static_render_config_arc_when_config_is_unchanged() {
        let mut state = RuntimeState::default();
        let first = reduce_cursor_event(&mut state, "n", event(5.0, 6.0), EventSource::External);
        let _ = reduce_cursor_event(
            &mut state,
            "n",
            event_with_location(5.0, 12.0, 116.0, 11, 10, 20),
            EventSource::External,
        );
        let second = reduce_cursor_event(
            &mut state,
            "n",
            event_with_location(5.0, 12.0, 132.0, 11, 10, 20),
            EventSource::AnimationTick,
        );

        let first_static = draw_frame(&first)
            .map(|frame| Arc::clone(&frame.static_config))
            .expect("first transition should draw");
        let second_static = draw_frame(&second)
            .map(|frame| Arc::clone(&frame.static_config))
            .expect("second transition should draw");
        assert!(Arc::ptr_eq(&first_static, &second_static));
    }
}

mod mode_specific_transitions {
    use super::*;

    fn insert_immediate_snap() -> (RuntimeState, CursorTransition) {
        let (mut state, _) = initialized_runtime("n", |state| {
            state.config.delay_event_to_smear = 0.0;
            state.config.smear_insert_mode = true;
            state.config.animate_in_insert_mode = false;
        });
        let transition = reduce_cursor_event(
            &mut state,
            "i",
            event_with_location(5.0, 20.0, 116.0, 17, 10, 20),
            EventSource::External,
        );
        (state, transition)
    }

    fn cmdline_boundary_transition() -> (RuntimeState, CursorTransition) {
        let (mut state, _) = initialized_runtime("n", |state| {
            state.config.delay_event_to_smear = 0.0;
            state.config.smear_to_cmd = true;
            state.config.animate_command_line = false;
        });
        let boundary = reduce_cursor_event(
            &mut state,
            "c",
            event_with_location(5.0, 12.0, 116.0, 18, 10, 20),
            EventSource::External,
        );
        (state, boundary)
    }

    fn text_mutation_snap() -> (RuntimeState, CursorTransition) {
        let (mut state, _) = animating_runtime_after_kickoff(|state| {
            state.config.smear_insert_mode = true;
            state.config.animate_in_insert_mode = true;
        });
        let transition = reduce_cursor_event(
            &mut state,
            "i",
            text_mutation_event(5.0, 20.0, 132.0),
            EventSource::External,
        );
        (state, transition)
    }

    fn animation_tick_retarget() -> (RuntimeState, u64, CursorTransition) {
        let (mut state, _) = animating_runtime_after_kickoff(|state| {
            state.config.delay_event_to_smear = 0.0;
        });
        let before_retarget = reduce_cursor_event(
            &mut state,
            "n",
            event_with_location(5.0, 12.0, 124.0, 11, 10, 20),
            EventSource::AnimationTick,
        );
        let before_retarget_epoch = draw_frame(&before_retarget)
            .map(|frame| frame.retarget_epoch)
            .expect("expected pre-retarget animation tick to draw");
        let retarget = reduce_cursor_event(
            &mut state,
            "n",
            CursorEventContext {
                row: 12.0,
                col: 24.0,
                now_ms: 132.0,
                seed: 12,
                cursor_location: CursorLocation::new(10, 20, 1, 2),
                scroll_shift: None,
                semantic_event: SemanticEvent::FrameCommitted,
            },
            EventSource::AnimationTick,
        );
        (state, before_retarget_epoch, retarget)
    }

    #[test]
    fn cmdline_external_events_progress_after_a_settle_tick() {
        let mut state = RuntimeState::default();
        state.config.delay_event_to_smear = 0.0;
        state.config.smear_to_cmd = true;

        let _ = reduce_cursor_event(&mut state, "c", event(5.0, 6.0), EventSource::External);
        let _ = reduce_cursor_event(
            &mut state,
            "c",
            event_with_location(5.0, 12.0, 116.0, 11, 10, 20),
            EventSource::External,
        );
        let _ = reduce_cursor_event(
            &mut state,
            "c",
            event_with_location(5.0, 12.0, 118.0, 11, 10, 20),
            EventSource::AnimationTick,
        );
        let effects = reduce_cursor_event(
            &mut state,
            "c",
            event_with_location(5.0, 16.0, 132.0, 12, 10, 20),
            EventSource::External,
        );

        assert!(matches!(render_action(&effects), RenderAction::Draw(_)));
        assert!(state.is_animating());
    }

    #[test]
    fn insert_mode_immediate_snap_emits_a_discontinuous_jump_bridge_frame() {
        let (state, transition) = insert_immediate_snap();
        let frame = draw_frame(&transition)
            .expect("insert immediate mode should still emit a jump bridge frame");

        assert_eq!(transition.motion_class, MotionClass::DiscontinuousJump);
        assert!(transition.should_schedule_next_animation);
        assert!(state.is_draining());
        assert!(frame.step_samples.len() >= 3);
    }

    #[test]
    fn insert_mode_immediate_snap_updates_the_target_without_disabling_insert_smear() {
        let (state, _) = insert_immediate_snap();
        assert_eq!(
            state.target_position(),
            Point {
                row: 5.0,
                col: 20.0
            }
        );
    }

    #[test]
    fn insert_mode_motion_still_animates_without_text_mutation() {
        let (mut state, _) = initialized_runtime("n", |state| {
            state.config.delay_event_to_smear = 0.0;
            state.config.smear_insert_mode = true;
            state.config.animate_in_insert_mode = true;
        });
        let transition = reduce_cursor_event(
            &mut state,
            "i",
            event_with_location(5.0, 20.0, 116.0, 17, 10, 20),
            EventSource::External,
        );

        assert!(matches!(render_action(&transition), RenderAction::Draw(_)));
        assert!(state.is_animating());
    }

    #[test]
    fn text_mutation_snap_clears_existing_smear_instead_of_animating() {
        let (state, transition) = text_mutation_snap();

        assert!(matches!(render_action(&transition), RenderAction::ClearAll));
        assert!(!transition.should_schedule_next_animation);
        assert!(!state.is_animating());
        assert_eq!(
            state.target_position(),
            Point {
                row: 5.0,
                col: 20.0,
            }
        );
    }

    #[test]
    fn cmdline_boundary_snap_emits_an_immediate_jump_bridge_frame() {
        let (state, boundary) = cmdline_boundary_transition();
        let boundary_frame = draw_frame(&boundary)
            .expect("cmdline boundary snap should emit an immediate jump bridge frame");

        assert_eq!(boundary.motion_class, MotionClass::DiscontinuousJump);
        assert!(boundary.should_schedule_next_animation);
        assert!(state.is_draining());
        assert!(boundary_frame.step_samples.len() >= 3);
    }

    #[test]
    fn intra_cmdline_motion_continues_animating_after_the_boundary_snap() {
        let (mut state, _) = cmdline_boundary_transition();
        let within_cmdline = reduce_cursor_event(
            &mut state,
            "c",
            event_with_location(5.0, 18.0, 132.0, 19, 10, 20),
            EventSource::External,
        );

        assert!(
            matches!(render_action(&within_cmdline), RenderAction::Draw(_)),
            "movement while already in cmdline should keep animating"
        );
        assert!(state.is_animating());
    }

    #[test]
    fn animation_tick_retargets_advance_the_retarget_epoch_before_the_next_draw() {
        let (_, before_retarget_epoch, effects) = animation_tick_retarget();
        let after_retarget_epoch = draw_frame(&effects)
            .map(|frame| frame.retarget_epoch)
            .expect("expected retarget animation tick to draw");

        assert!(
            after_retarget_epoch > before_retarget_epoch,
            "retarget epoch should advance so draw acknowledgement cache does not suppress first retarget frame"
        );
    }

    #[test]
    fn animation_tick_retargets_update_target_and_location_while_keeping_frames_scheduled() {
        let (state, _, effects) = animation_tick_retarget();

        assert!(matches!(render_action(&effects), RenderAction::Draw(_)));
        assert!(effects.should_schedule_next_animation);
        assert!(state.is_animating() || state.is_draining());
        assert_eq!(
            state.target_position(),
            Point {
                row: 12.0,
                col: 24.0
            }
        );
        assert_eq!(
            state.tracked_location(),
            Some(CursorLocation::new(10, 20, 1, 2))
        );
    }
}

mod trajectory_goldens {
    use super::*;

    #[test]
    fn rapid_horizontal_motion() {
        let mut state = RuntimeState::default();
        state.config.delay_event_to_smear = 0.0;
        state.config.simulation_hz = 240.0;
        state.config.max_simulation_steps_per_frame = 16;

        let steps = [
            external_step(5.0, 6.0, 100.0, 7, 10, 20),
            external_step(5.0, 16.0, 108.0, 8, 10, 20),
            animation_tick_step(5.0, 16.0, 116.0, 9, 10, 20),
            external_step(5.0, 24.0, 120.0, 10, 10, 20),
            animation_tick_step(5.0, 24.0, 128.0, 11, 10, 20),
            external_step(5.0, 36.0, 132.0, 12, 10, 20),
            animation_tick_step(5.0, 36.0, 140.0, 13, 10, 20),
            animation_tick_step(5.0, 36.0, 148.0, 14, 10, 20),
        ];

        let fingerprint = trajectory_fingerprint(&mut state, "n", &steps);
        let replay = trajectory_fingerprint_with_fresh_state(&state, "n", &steps);
        assert_eq!(fingerprint, replay, "trajectory must be deterministic");
        assert_eq!(
            fingerprint, 10_543_951_919_560_940_113_u64,
            "update golden hash if this change is intentional"
        );
    }

    #[test]
    fn diagonal_zig_zag_motion() {
        let mut state = RuntimeState::default();
        state.config.delay_event_to_smear = 0.0;
        state.config.simulation_hz = 240.0;
        state.config.max_simulation_steps_per_frame = 16;

        let steps = [
            external_step(5.0, 6.0, 100.0, 21, 10, 20),
            external_step(9.0, 12.0, 108.0, 22, 10, 20),
            animation_tick_step(9.0, 12.0, 116.0, 23, 10, 20),
            external_step(4.0, 20.0, 124.0, 24, 10, 20),
            animation_tick_step(4.0, 20.0, 132.0, 25, 10, 20),
            external_step(10.0, 28.0, 140.0, 26, 10, 20),
            animation_tick_step(10.0, 28.0, 148.0, 27, 10, 20),
            animation_tick_step(10.0, 28.0, 156.0, 28, 10, 20),
        ];

        let fingerprint = trajectory_fingerprint(&mut state, "n", &steps);
        let replay = trajectory_fingerprint_with_fresh_state(&state, "n", &steps);
        assert_eq!(fingerprint, replay, "trajectory must be deterministic");
        assert_eq!(
            fingerprint, 930_688_188_185_069_218_u64,
            "update golden hash if this change is intentional"
        );
    }

    #[test]
    fn window_and_buffer_switch_motion() {
        let mut state = RuntimeState::default();
        state.config.delay_event_to_smear = 0.0;
        state.config.simulation_hz = 240.0;
        state.config.max_simulation_steps_per_frame = 16;
        state.config.smear_between_windows = true;
        state.config.smear_between_buffers = true;

        let steps = [
            external_step(5.0, 6.0, 100.0, 31, 10, 20),
            external_step(20.0, 30.0, 108.0, 32, 11, 21),
            animation_tick_step(20.0, 30.0, 116.0, 33, 11, 21),
            external_step(9.0, 14.0, 124.0, 34, 10, 20),
            animation_tick_step(9.0, 14.0, 132.0, 35, 10, 20),
            external_step(24.0, 4.0, 140.0, 36, 12, 22),
            animation_tick_step(24.0, 4.0, 148.0, 37, 12, 22),
            animation_tick_step(24.0, 4.0, 156.0, 38, 12, 22),
        ];

        let fingerprint = trajectory_fingerprint(&mut state, "n", &steps);
        let replay = trajectory_fingerprint_with_fresh_state(&state, "n", &steps);
        assert_eq!(fingerprint, replay, "trajectory must be deterministic");
        assert_eq!(
            fingerprint, 17_834_402_511_405_591_660_u64,
            "update golden hash if this change is intentional"
        );
    }

    #[test]
    fn scroll_while_animating() {
        let mut state = RuntimeState::default();
        state.config.delay_event_to_smear = 0.0;
        state.config.simulation_hz = 240.0;
        state.config.max_simulation_steps_per_frame = 16;

        let steps = [
            external_step(15.0, 6.0, 100.0, 41, 10, 20),
            external_step(24.0, 6.0, 108.0, 42, 10, 20),
            animation_tick_step(24.0, 6.0, 116.0, 43, 10, 20),
            external_with_scroll_step(
                24.0,
                12.0,
                124.0,
                44,
                10,
                20,
                ScrollShift {
                    shift: 2.0,
                    min_row: 1.0,
                    max_row: 60.0,
                },
            ),
            animation_tick_step(24.0, 12.0, 132.0, 45, 10, 20),
            animation_tick_with_scroll_step(
                24.0,
                12.0,
                140.0,
                46,
                10,
                20,
                ScrollShift {
                    shift: 1.0,
                    min_row: 1.0,
                    max_row: 60.0,
                },
            ),
            animation_tick_step(24.0, 12.0, 148.0, 47, 10, 20),
        ];

        let fingerprint = trajectory_fingerprint(&mut state, "n", &steps);
        let replay = trajectory_fingerprint_with_fresh_state(&state, "n", &steps);
        assert_eq!(fingerprint, replay, "trajectory must be deterministic");
        assert_eq!(
            fingerprint, 9_334_719_745_673_801_694_u64,
            "update golden hash if this change is intentional"
        );
    }
}

mod fixed_step_stability {
    use super::*;

    #[derive(Clone, Copy, Debug)]
    struct StabilitySummary {
        settle_at_ms: f64,
        peak_distance: f64,
        final_center: Point,
    }

    fn run_fps_stability_scenario(render_fps: f64) -> StabilitySummary {
        let mut state = RuntimeState::default();
        state.config.fps = render_fps;
        state.config.time_interval = RuntimeConfig::interval_ms_for_fps(render_fps);
        state.config.simulation_hz = 240.0;
        state.config.max_simulation_steps_per_frame = 16;
        state.config.delay_event_to_smear = 0.0;
        state.config.stop_distance_enter = 0.08;
        state.config.stop_distance_exit = 0.16;
        state.config.stop_velocity_enter = 0.05;
        state.config.stop_hold_frames = 2;

        let _ = reduce_cursor_event(
            &mut state,
            "n",
            event_with_location(5.0, 6.0, 100.0, 51, 10, 20),
            EventSource::External,
        );
        let _ = reduce_cursor_event(
            &mut state,
            "n",
            event_with_location(5.0, 56.0, 108.0, 52, 10, 20),
            EventSource::External,
        );

        let frame_dt = RuntimeConfig::interval_ms_for_fps(render_fps);
        let target = Point {
            row: 5.0,
            col: 56.0,
        };
        let mut now_ms = 108.0;
        let mut peak_distance = 0.0_f64;
        let mut settle_at_ms = now_ms;

        for _ in 0..600 {
            now_ms += frame_dt;
            let _ = reduce_cursor_event(
                &mut state,
                "n",
                event_with_location(target.row, target.col, now_ms, 53, 10, 20),
                EventSource::AnimationTick,
            );

            let center = trajectory_center(&state);
            peak_distance = peak_distance.max((center.col - target.col).abs());
            if !state.is_animating() && !state.is_settling() {
                settle_at_ms = now_ms;
                break;
            }
        }

        StabilitySummary {
            settle_at_ms,
            peak_distance,
            final_center: trajectory_center(&state),
        }
    }

    #[test]
    fn fixed_simulation_rate_keeps_motion_stable_across_render_fps() {
        let mut summaries = Vec::new();
        for fps in [60.0_f64, 90.0_f64, 120.0_f64, 144.0_f64] {
            summaries.push(run_fps_stability_scenario(fps));
        }

        let settle_min = summaries
            .iter()
            .map(|summary| summary.settle_at_ms)
            .fold(f64::INFINITY, f64::min);
        let settle_max = summaries
            .iter()
            .map(|summary| summary.settle_at_ms)
            .fold(0.0_f64, f64::max);
        let peak_min = summaries
            .iter()
            .map(|summary| summary.peak_distance)
            .fold(f64::INFINITY, f64::min);
        let peak_max = summaries
            .iter()
            .map(|summary| summary.peak_distance)
            .fold(0.0_f64, f64::max);

        for summary in &summaries {
            assert!((summary.final_center.row - 5.5).abs() <= 0.2);
            assert!((summary.final_center.col - 56.5).abs() <= 0.2);
        }
        assert!(
            settle_max - settle_min <= 220.0,
            "settle timing drifted too much across fps: min={settle_min} max={settle_max} summaries={summaries:?}"
        );
        assert!(
            peak_max - peak_min <= 20.0,
            "peak motion envelope drifted too much across fps: min={peak_min} max={peak_max} summaries={summaries:?}"
        );
    }
}

mod property_invariants {
    use super::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(96))]

        #[test]
        fn prop_window_change_clears_when_smear_between_windows_disabled(
            start_row in 1_u16..120,
            start_col in 1_u16..220,
            switched_row in 120_u16..280,
            switched_col in 120_u16..280,
            initial_window in any::<i64>(),
            initial_buffer in any::<i64>(),
            window_delta in 1_i16..32,
            initial_seed in any::<u32>(),
            switched_seed in any::<u32>(),
        ) {
            let mut state = RuntimeState::default();
            state.config.smear_between_windows = false;
            state.config.smear_between_buffers = true;
            state.config.jump_cues_enabled = false;

            let initial = event_with_location(
                f64::from(start_row),
                f64::from(start_col),
                100.0,
                initial_seed,
                initial_window,
                initial_buffer,
            );
            let _ = reduce_cursor_event(&mut state, "n", initial, EventSource::External);

            let switched = event_with_location(
                f64::from(switched_row),
                f64::from(switched_col),
                116.0,
                switched_seed,
                initial_window.wrapping_add(i64::from(window_delta)),
                initial_buffer,
            );
            let effects = reduce_cursor_event(&mut state, "n", switched, EventSource::External);
            prop_assert!(matches!(render_action(&effects), RenderAction::ClearAll));
            prop_assert_eq!(
                render_cleanup_action(&effects),
                RenderCleanupAction::Schedule
            );
        }

        #[test]
        fn prop_repeated_window_switches_do_not_clear_when_smear_between_windows_enabled(
            switches in vec((20_u16..260, 20_u16..260, any::<u32>()), 1..96),
        ) {
            let mut state = RuntimeState::default();
            state.config.smear_between_windows = true;

            let initial = event_with_location(5.0, 6.0, 100.0, 7, 10, 20);
            let _ = reduce_cursor_event(&mut state, "n", initial, EventSource::External);

            let mut now_ms = 120.0;
            for (index, (row, col, seed)) in switches.into_iter().enumerate() {
                let window_handle = if index % 2 == 0 { 100_i64 } else { 101_i64 };
                let step_event = event_with_location(
                    f64::from(row),
                    f64::from(col),
                    now_ms,
                    seed,
                    window_handle,
                    20,
                );
                let _ = reduce_cursor_event(&mut state, "n", step_event, EventSource::External);
                let effects = reduce_cursor_event(
                    &mut state,
                    "n",
                    event_with_location(
                        f64::from(row),
                        f64::from(col),
                        now_ms + 2.0,
                        seed,
                        window_handle,
                        20,
                    ),
                    EventSource::AnimationTick,
                );
                prop_assert!(
                    !matches!(render_action(&effects), RenderAction::ClearAll),
                    "unexpected clear-all at step {index}"
                );
                prop_assert_ne!(
                    render_cleanup_action(&effects),
                    RenderCleanupAction::Schedule,
                    "unexpected cleanup scheduling at step {}",
                    index
                );
                now_ms += 16.0;
            }
        }

        #[test]
        fn prop_idle_animation_ticks_are_idempotent(
            ticks in vec((0_u16..320, 0_u16..320, any::<u32>()), 1..96),
        ) {
            let mut state = RuntimeState::default();
            state.mark_initialized();
            state.stop_animation();
            let tracked_before = state.tracked_location();
            let last_tick_before = state.last_tick_ms();

            let mut now_ms = 100.0;
            for (row, col, seed) in ticks {
                let tick = event_with_location(
                    f64::from(row),
                    f64::from(col),
                    now_ms,
                    seed,
                    10,
                    20,
                );
                let effects = reduce_cursor_event(&mut state, "n", tick, EventSource::AnimationTick);
                prop_assert!(matches!(render_action(&effects), RenderAction::Noop));
                prop_assert_eq!(
                    render_cleanup_action(&effects),
                    RenderCleanupAction::NoAction
                );
                prop_assert!(state.is_initialized());
                prop_assert!(!state.is_animating());
                prop_assert_eq!(state.tracked_location(), tracked_before.clone());
                prop_assert_eq!(state.last_tick_ms(), last_tick_before);
                now_ms += 16.0;
            }
        }

        #[test]
        fn prop_clear_all_and_external_noop_always_emit_cleanup_intent(
            steps in vec((any::<bool>(), 0_u16..320, 0_u16..320, any::<u32>(), any::<i64>(), any::<i64>()), 1..160),
        ) {
            let mut state = RuntimeState::default();
            let mut now_ms = 100.0;

            for (is_external, row, col, seed, window_handle, buffer_handle) in steps {
                let source = if is_external {
                    EventSource::External
                } else {
                    EventSource::AnimationTick
                };
                let step_event = event_with_location(
                    f64::from(row),
                    f64::from(col),
                    now_ms,
                    seed,
                    window_handle,
                    buffer_handle,
                );
                let effects = reduce_cursor_event(&mut state, "n", step_event, source);
                if matches!(render_action(&effects), RenderAction::ClearAll) {
                    prop_assert_ne!(
                        render_cleanup_action(&effects),
                        RenderCleanupAction::NoAction
                    );
                }
                if source == EventSource::External
                    && matches!(render_action(&effects), RenderAction::Noop)
                {
                    prop_assert_ne!(
                        render_cleanup_action(&effects),
                        RenderCleanupAction::NoAction
                    );
                }

                now_ms += 8.0;
            }
        }
    }
}

use std::borrow::Borrow;

use super::CursorEventContext;
use super::CursorTransition;
use super::CursorVisibilityEffect;
use super::EventSource;
use super::MotionClass;
use super::RenderAllocationPolicy;
use super::RenderCleanupAction;
use super::decision::CursorTransitions;
use super::frame::apply_scroll_shift_to_state;
use super::frame::build_render_frame;
use super::frame::clamp_row_to_window;
use super::frame::next_animation_deadline_from_clock;
use super::frame::next_animation_deadline_from_settling;
use super::frame::reset_animation_timing;
use super::frame::step_input;
use super::policy::classify_target_transition;
use super::policy::external_mode_ignores_cursor;
use super::policy::external_mode_requires_immediate_movement;
use super::policy::external_mode_requires_jump;
use crate::animation::corners_for_cursor;
use crate::animation::corners_for_render;
use crate::animation::outside_stop_exit;
use crate::animation::simulate_step;
use crate::animation::stop_metrics;
use crate::animation::within_stop_enter;
use crate::core::state::SemanticEvent;
use crate::state::CursorShape;
use crate::state::RuntimeState;
use crate::types::EPSILON;
use crate::types::Point;
use crate::types::RenderStepSample;
use nvimrs_nvim_utils::mode::is_cmdline_mode;

const DEFAULT_TAIL_DURATION_MS: f64 = 198.0;
const DURATION_SCALE_MIN: f64 = 0.40;
const DURATION_SCALE_MAX: f64 = 2.50;
const DURATION_SCALE_EXPONENT: f64 = 0.85;
const SHEATH_BASE_LIFETIME_MS: f64 = 40.0;
const CORE_BASE_LIFETIME_MS: f64 = 112.0;
const FILAMENT_BASE_LIFETIME_MS: f64 = 252.0;
const SHEATH_MIN_SUPPORT_STEPS: u32 = 2;
const CORE_MIN_SUPPORT_STEPS: u32 = 4;
const FILAMENT_MIN_SUPPORT_STEPS: u32 = 7;
const JUMP_BRIDGE_MIN_SEGMENTS: usize = 2;
const JUMP_BRIDGE_MAX_SEGMENTS: usize = 8;
const JUMP_BRIDGE_SEGMENT_LENGTH_CELLS: f64 = 6.0;

fn tail_duration_support_scale(tail_duration_ms: f64) -> f64 {
    if tail_duration_ms.is_finite() {
        (tail_duration_ms / DEFAULT_TAIL_DURATION_MS).clamp(DURATION_SCALE_MIN, DURATION_SCALE_MAX)
    } else {
        1.0
    }
    .powf(DURATION_SCALE_EXPONENT)
}

fn support_steps_for_lifetime(lifetime_ms: f64, min_support_steps: u32, simulation_hz: f64) -> u32 {
    let step_ms = if simulation_hz.is_finite() {
        (1000.0 / simulation_hz.max(1.0)).max(1.0)
    } else {
        1000.0 / 120.0
    };
    ((lifetime_ms / step_ms).round() as u32)
        .max(min_support_steps)
        .max(1)
}

fn planner_tail_drain_steps(state: &RuntimeState) -> u32 {
    // settle-time drain must follow the same support-window envelope as the planner's
    // latent-field aging. If these diverge, reducer lifecycle truth and render decay truth drift.
    let support_scale = tail_duration_support_scale(state.config.tail_duration_ms);
    [
        support_steps_for_lifetime(
            SHEATH_BASE_LIFETIME_MS * support_scale,
            SHEATH_MIN_SUPPORT_STEPS,
            state.config.simulation_hz,
        ),
        support_steps_for_lifetime(
            CORE_BASE_LIFETIME_MS * support_scale,
            CORE_MIN_SUPPORT_STEPS,
            state.config.simulation_hz,
        ),
        support_steps_for_lifetime(
            FILAMENT_BASE_LIFETIME_MS * support_scale,
            FILAMENT_MIN_SUPPORT_STEPS,
            state.config.simulation_hz,
        ),
    ]
    .into_iter()
    .max()
    .unwrap_or(1)
}

fn lerp_point(from: Point, to: Point, t: f64) -> Point {
    Point {
        row: from.row + ((to.row - from.row) * t),
        col: from.col + ((to.col - from.col) * t),
    }
}

fn jump_bridge_step_samples(
    state: &RuntimeState,
    from_position: Point,
    to_position: Point,
    vertical_bar: bool,
    horizontal_bar: bool,
) -> Vec<RenderStepSample> {
    let display_distance =
        from_position.display_distance(to_position, state.config.block_aspect_ratio);
    let raw_segments = if display_distance.is_finite() {
        (display_distance / JUMP_BRIDGE_SEGMENT_LENGTH_CELLS).ceil() as usize
    } else {
        JUMP_BRIDGE_MIN_SEGMENTS
    };
    let segment_count = raw_segments.clamp(JUMP_BRIDGE_MIN_SEGMENTS, JUMP_BRIDGE_MAX_SEGMENTS);
    let dt_ms = (state.config.jump_cue_duration_ms / segment_count as f64)
        .max(state.config.simulation_step_interval_ms().max(1.0))
        .max(1.0);
    let mut samples = Vec::with_capacity(segment_count + 1);
    for step_index in 0..=segment_count {
        let t = step_index as f64 / segment_count as f64;
        let position = lerp_point(from_position, to_position, t);
        samples.push(RenderStepSample::new(
            corners_for_cursor(position.row, position.col, vertical_bar, horizontal_bar),
            dt_ms,
        ));
    }
    samples
}

#[derive(Clone, Copy)]
struct JumpFrameSpec {
    event_now_ms: f64,
    from_position: Point,
    to_position: Point,
    vertical_bar: bool,
    horizontal_bar: bool,
    motion_class: MotionClass,
}

fn draw_discontinuous_jump_frame(
    state: &mut RuntimeState,
    mode: &str,
    spec: JumpFrameSpec,
) -> CursorTransition {
    let frame = build_render_frame(
        state,
        mode,
        state.current_corners(),
        jump_bridge_step_samples(
            state,
            spec.from_position,
            spec.to_position,
            spec.vertical_bar,
            spec.horizontal_bar,
        ),
        0,
        spec.to_position,
        spec.vertical_bar,
    );
    state.start_tail_drain(planner_tail_drain_steps(state));
    state.set_last_tick_ms(Some(spec.event_now_ms));
    let next_animation_at_ms = Some(next_animation_deadline_from_clock(state, spec.event_now_ms));
    CursorTransitions::draw(
        mode,
        frame,
        true,
        RenderAllocationPolicy::BootstrapIfPoolEmpty,
    )
    .with_motion_class(spec.motion_class)
    .with_next_animation_at_ms(next_animation_at_ms)
    .with_render_cleanup_action(RenderCleanupAction::Invalidate)
}

fn promote_settled_target(state: &mut RuntimeState, now_ms: f64) {
    state.start_animation_towards_target();
    state.set_last_tick_ms(Some(now_ms));
    state.reset_settle_probe();
}

fn waiting_for_settled_target(
    state: &RuntimeState,
    mode: &str,
    allow_real_cursor_updates: bool,
    motion_class: MotionClass,
    now_ms: f64,
) -> CursorTransition {
    CursorTransitions::noop(mode, allow_real_cursor_updates)
        .with_motion_class(motion_class)
        .with_schedule_next_animation(true)
        .with_next_animation_at_ms(next_animation_deadline_from_settling(state, now_ms))
        .with_cursor_visibility(CursorVisibilityEffect::Show)
        .with_render_cleanup_action(RenderCleanupAction::Invalidate)
}

fn draw_drain_frame(
    state: &mut RuntimeState,
    mode: &str,
    event_now_ms: f64,
    target_position: Point,
    vertical_bar: bool,
) -> CursorTransition {
    let allow_real_cursor_updates = !state.config.hide_target_hack;
    let configured_interval = state.config.time_interval.max(1.0);
    let elapsed_ms = state
        .last_tick_ms()
        .map_or(configured_interval, |previous| {
            (event_now_ms - previous).max(0.0)
        });
    state.set_last_tick_ms(Some(event_now_ms));
    state.push_simulation_elapsed(elapsed_ms);

    let simulation_step_ms = state.config.simulation_step_interval_ms().max(1.0);
    let max_simulation_steps =
        usize::try_from(state.config.max_simulation_steps_per_frame).unwrap_or(usize::MAX);
    let mut planner_idle_steps = 0_u32;
    let mut executed_steps = 0_usize;
    while executed_steps < max_simulation_steps && state.consume_tail_drain_step(simulation_step_ms)
    {
        let particles = state.take_particles();
        let mut drain_input = step_input(
            state,
            mode,
            simulation_step_ms,
            vertical_bar,
            false,
            particles,
        );
        // tail drain should age already-emitted particles, but it must not emit new
        // particles after motion has settled or the visual tail can linger as a frozen cloud.
        drain_input.particles_enabled = false;
        let step_output = simulate_step(drain_input);
        state.apply_step_output(step_output);
        planner_idle_steps = planner_idle_steps.saturating_add(1);
        executed_steps = executed_steps.saturating_add(1);
    }
    let should_schedule_next_animation = state.is_draining();
    let next_animation_at_ms = should_schedule_next_animation
        .then(|| next_animation_deadline_from_clock(state, event_now_ms));

    if !should_schedule_next_animation {
        // Surprising: keep-warm cleanup intentionally preserves hidden cached windows. Terminal
        // tail drain therefore must collapse into an explicit clear instead of relying on a later
        // soft cleanup pass, or the last smear frame can remain visible until unrelated ingress.
        return CursorTransitions::clear_all(mode, allow_real_cursor_updates)
            .with_render_cleanup_action(RenderCleanupAction::Schedule);
    }

    if planner_idle_steps == 0 {
        return CursorTransitions::noop(mode, allow_real_cursor_updates)
            .with_schedule_next_animation(should_schedule_next_animation)
            .with_next_animation_at_ms(next_animation_at_ms)
            .with_cursor_visibility(CursorVisibilityEffect::Show)
            .with_render_cleanup_action(RenderCleanupAction::Invalidate);
    }

    let current_corners = state.current_corners();
    let target_corners = state.target_corners();
    let render_corners = corners_for_render(&state.config, &current_corners, &target_corners);
    let frame = build_render_frame(
        state,
        mode,
        render_corners,
        Vec::new(),
        planner_idle_steps,
        target_position,
        vertical_bar,
    );
    CursorTransitions::draw(
        mode,
        frame,
        should_schedule_next_animation,
        RenderAllocationPolicy::ReuseOnly,
    )
    .with_next_animation_at_ms(next_animation_at_ms)
    .with_render_cleanup_action(RenderCleanupAction::Invalidate)
}

pub(crate) fn reduce_cursor_event(
    state: &mut RuntimeState,
    mode: &str,
    event: impl Borrow<CursorEventContext>,
    source: EventSource,
) -> CursorTransition {
    let event = event.borrow();
    let allow_real_cursor_updates = !state.config.hide_target_hack;
    if !state.is_enabled() {
        state.clear_pending_target();
        state.stop_animation();
        reset_animation_timing(state);
        return CursorTransitions::clear_all(mode, allow_real_cursor_updates)
            .with_render_cleanup_action(RenderCleanupAction::Invalidate);
    }
    state.refresh_jump_cues(event.now_ms);

    let vertical_bar = state.config.cursor_is_vertical_bar(mode);
    let horizontal_bar = state.config.cursor_is_horizontal_bar(mode);
    let cursor_shape = CursorShape::new(vertical_bar, horizontal_bar);
    let event_target = Point {
        row: event.row,
        col: event.col,
    };
    let in_cmdline_mode = is_cmdline_mode(mode);
    let transitioned_to_or_from_cmdline = state
        .last_mode_was_cmdline()
        .is_some_and(|was_cmdline| was_cmdline != in_cmdline_mode);
    state.set_last_mode_was_cmdline(in_cmdline_mode);
    let tick_retarget = matches!(source, EventSource::AnimationTick)
        && (state.is_animating() || state.is_settling() || state.is_draining())
        && state.target_position().distance_squared(event_target) > EPSILON;
    let mut target_position = match source {
        EventSource::AnimationTick if tick_retarget => event_target,
        EventSource::AnimationTick => state.target_position(),
        EventSource::External => event_target,
    };
    let (window_changed, buffer_changed) =
        state
            .tracked_location_ref()
            .map_or((false, false), |tracked| {
                (
                    tracked.window_handle != event.cursor_location.window_handle,
                    tracked.buffer_handle != event.cursor_location.buffer_handle,
                )
            });
    let previous_target = state.target_position();
    let previous_location = state.tracked_location_ref().cloned();
    let mut motion_class = MotionClass::Continuous;

    match source {
        EventSource::External => {
            if external_mode_ignores_cursor(&state.config, mode) {
                return CursorTransitions::noop(mode, allow_real_cursor_updates)
                    .with_cursor_visibility(CursorVisibilityEffect::Show)
                    .with_render_cleanup_action(RenderCleanupAction::Schedule);
            }
            if event.semantic_event == SemanticEvent::TextMutatedAtCursorContext {
                state.jump_and_stop_animation(
                    target_position,
                    cursor_shape,
                    &event.cursor_location,
                );
                return CursorTransitions::clear_all(mode, allow_real_cursor_updates)
                    .with_render_cleanup_action(RenderCleanupAction::Schedule);
            }
            if external_mode_requires_immediate_movement(
                &state.config,
                mode,
                transitioned_to_or_from_cmdline,
            ) {
                motion_class = MotionClass::DiscontinuousJump;
                let jump_origin = previous_target;
                if let Some(from_location) = previous_location.as_ref() {
                    state.record_jump_cue(
                        jump_origin,
                        from_location,
                        target_position,
                        &event.cursor_location,
                        event.now_ms,
                    );
                }
                state.jump_and_stop_animation(
                    target_position,
                    cursor_shape,
                    &event.cursor_location,
                );
                return draw_discontinuous_jump_frame(
                    state,
                    mode,
                    JumpFrameSpec {
                        event_now_ms: event.now_ms,
                        from_position: jump_origin,
                        to_position: target_position,
                        vertical_bar,
                        horizontal_bar,
                        motion_class,
                    },
                );
            }
            if external_mode_requires_jump(&state.config, mode) {
                // Mode-forced jumps update position/target but preserve in-flight motion.
                motion_class = MotionClass::DiscontinuousJump;
                let jump_origin = previous_target;
                if let Some(from_location) = previous_location.as_ref() {
                    state.record_jump_cue(
                        jump_origin,
                        from_location,
                        target_position,
                        &event.cursor_location,
                        event.now_ms,
                    );
                }
                state.jump_preserving_motion(target_position, cursor_shape, &event.cursor_location);
                return draw_discontinuous_jump_frame(
                    state,
                    mode,
                    JumpFrameSpec {
                        event_now_ms: event.now_ms,
                        from_position: jump_origin,
                        to_position: target_position,
                        vertical_bar,
                        horizontal_bar,
                        motion_class,
                    },
                );
            }
        }
        EventSource::AnimationTick => {}
    }

    if !state.is_initialized() {
        state.initialize_cursor(
            target_position,
            cursor_shape,
            event.seed,
            &event.cursor_location,
        );
        let frame = build_render_frame(
            state,
            mode,
            state.current_corners(),
            // Bootstrap the planner with a stationary head sample so the first frame renders
            // visible occupancy before the animation loop has emitted fixed-step samples.
            vec![RenderStepSample::new(
                state.current_corners(),
                state.config.simulation_step_interval_ms(),
            )],
            0,
            target_position,
            vertical_bar,
        );
        return CursorTransitions::draw(
            mode,
            frame,
            false,
            RenderAllocationPolicy::BootstrapIfPoolEmpty,
        )
        .with_render_cleanup_action(RenderCleanupAction::Schedule);
    }

    let should_update_target = matches!(source, EventSource::External) || tick_retarget;
    if should_update_target {
        if let Some(scroll_shift) = event.scroll_shift {
            apply_scroll_shift_to_state(state, vertical_bar, horizontal_bar, scroll_shift);
            target_position.row =
                clamp_row_to_window(target_position.row - scroll_shift.shift, scroll_shift);
        }

        let path_segmentation = classify_target_transition(
            state,
            window_changed,
            buffer_changed,
            target_position.row,
            target_position.col,
        );
        motion_class = path_segmentation.motion_class;
        if matches!(motion_class, MotionClass::DiscontinuousJump)
            && let Some(from_location) = previous_location.as_ref()
        {
            state.record_jump_cue(
                previous_target,
                from_location,
                target_position,
                &event.cursor_location,
                event.now_ms,
            );
            // large discontinuous moves still reset trail semantics, but they should
            // stay on the regular spring/comet pipeline unless policy requires an actual snap.
        }

        if path_segmentation.should_jump {
            state.jump_and_stop_animation(target_position, cursor_shape, &event.cursor_location);
            return CursorTransitions::clear_all(mode, allow_real_cursor_updates)
                .with_motion_class(motion_class)
                .with_render_cleanup_action(RenderCleanupAction::Schedule);
        }

        if path_segmentation.starts_new_trail_stroke {
            state.start_new_trail_stroke();
        }
        state.set_target(target_position, cursor_shape);
        state.update_tracking(&event.cursor_location);
    }

    let mut just_started = false;
    if matches!(source, EventSource::External) && !state.is_animating() {
        let metrics = stop_metrics(
            &state.current_corners(),
            &state.target_corners(),
            &state.velocity_corners(),
            state.config.block_aspect_ratio,
            state.particles(),
        );
        if outside_stop_exit(&state.config, metrics) {
            if state.is_settling() {
                state.refresh_settling_target(
                    target_position,
                    cursor_shape,
                    &event.cursor_location,
                    event.now_ms,
                );
            } else {
                state.begin_settling(
                    target_position,
                    cursor_shape,
                    &event.cursor_location,
                    event.now_ms,
                );
            }

            if state.should_promote_settled_target(
                event.now_ms,
                target_position,
                &event.cursor_location,
            ) {
                promote_settled_target(state, event.now_ms);
                just_started = true;
            } else {
                return waiting_for_settled_target(
                    state,
                    mode,
                    allow_real_cursor_updates,
                    motion_class,
                    event.now_ms,
                );
            }
        } else {
            state.clear_pending_target();
            state.settle_at_target();
            state.stop_animation();
            reset_animation_timing(state);
            // a same-target external ingress after drain does not supersede the last
            // rendered trail. Keep cleanup intent alive so the final smear frame still clears.
            return CursorTransitions::noop(mode, allow_real_cursor_updates)
                .with_motion_class(motion_class)
                .with_cursor_visibility(CursorVisibilityEffect::Show)
                .with_render_cleanup_action(RenderCleanupAction::Schedule);
        }
    }

    if matches!(source, EventSource::AnimationTick) && state.is_settling() {
        if state.should_promote_settled_target(
            event.now_ms,
            target_position,
            &event.cursor_location,
        ) {
            promote_settled_target(state, event.now_ms);
            just_started = true;
        } else {
            state.refresh_settling_target(
                target_position,
                cursor_shape,
                &event.cursor_location,
                event.now_ms,
            );
            return waiting_for_settled_target(
                state,
                mode,
                allow_real_cursor_updates,
                motion_class,
                event.now_ms,
            );
        }
    }

    if matches!(source, EventSource::AnimationTick) && state.is_draining() {
        return draw_drain_frame(state, mode, event.now_ms, target_position, vertical_bar);
    }

    match source {
        EventSource::AnimationTick if !state.is_animating() => {
            return CursorTransitions::noop(mode, allow_real_cursor_updates)
                .with_motion_class(motion_class)
                .with_cursor_visibility(CursorVisibilityEffect::Show);
        }
        EventSource::AnimationTick | EventSource::External => {}
    }

    let should_advance = match source {
        EventSource::AnimationTick => state.is_animating(),
        // External retargets while animating should still advance the simulation so
        // high-frequency cursor streams do not appear frozen between timer ticks.
        EventSource::External => just_started || state.is_animating(),
    };

    if should_advance {
        let configured_interval = state.config.time_interval.max(1.0);
        let step_interval = if just_started {
            configured_interval
        } else {
            state
                .last_tick_ms()
                .map_or(configured_interval, |previous| {
                    (event.now_ms - previous).max(0.0)
                })
        };
        state.set_last_tick_ms(Some(event.now_ms));

        state.push_simulation_elapsed(step_interval);
        let simulation_step_ms = state.config.simulation_step_interval_ms().max(1.0);
        let max_simulation_steps =
            usize::try_from(state.config.max_simulation_steps_per_frame).unwrap_or(usize::MAX);
        let mut executed_steps = 0_usize;
        let mut step_samples = Vec::<RenderStepSample>::with_capacity(max_simulation_steps);

        while executed_steps < max_simulation_steps
            && state.consume_simulation_step(simulation_step_ms)
        {
            let particles = state.take_particles();
            let step_input = step_input(
                state,
                mode,
                simulation_step_ms,
                vertical_bar,
                horizontal_bar,
                particles,
            );
            let step_output = simulate_step(step_input);
            state.apply_step_output(step_output);
            let current_corners = state.current_corners();
            let target_corners = state.target_corners();
            let render_corners =
                corners_for_render(&state.config, &current_corners, &target_corners);
            step_samples.push(RenderStepSample::new(render_corners, simulation_step_ms));
            executed_steps = executed_steps.saturating_add(1);
        }

        if executed_steps == 0 && just_started {
            let particles = state.take_particles();
            let step_input = step_input(
                state,
                mode,
                simulation_step_ms,
                vertical_bar,
                horizontal_bar,
                particles,
            );
            let step_output = simulate_step(step_input);
            state.apply_step_output(step_output);
            let current_corners = state.current_corners();
            let target_corners = state.target_corners();
            let render_corners =
                corners_for_render(&state.config, &current_corners, &target_corners);
            step_samples.push(RenderStepSample::new(render_corners, simulation_step_ms));
        }

        let current_corners = state.current_corners();
        let target_corners = state.target_corners();
        let velocity_corners = state.velocity_corners();
        let metrics = stop_metrics(
            &current_corners,
            &target_corners,
            &velocity_corners,
            state.config.block_aspect_ratio,
            state.particles(),
        );
        if state.note_settle_probe(within_stop_enter(&state.config, metrics)) {
            state.settle_at_target();
            state.start_tail_drain(planner_tail_drain_steps(state));
            let current_corners = state.current_corners();
            let target_corners = state.target_corners();
            let render_corners =
                corners_for_render(&state.config, &current_corners, &target_corners);
            let frame = build_render_frame(
                state,
                mode,
                render_corners,
                step_samples,
                0,
                target_position,
                vertical_bar,
            );
            let next_animation_at_ms =
                Some(next_animation_deadline_from_clock(state, event.now_ms));
            return CursorTransitions::draw(mode, frame, true, RenderAllocationPolicy::ReuseOnly)
                .with_motion_class(motion_class)
                .with_next_animation_at_ms(next_animation_at_ms)
                .with_render_cleanup_action(RenderCleanupAction::Invalidate);
        }

        let current_corners = state.current_corners();
        let target_corners = state.target_corners();
        let render_corners = corners_for_render(&state.config, &current_corners, &target_corners);
        let frame = build_render_frame(
            state,
            mode,
            render_corners,
            step_samples,
            0,
            target_position,
            vertical_bar,
        );
        let next_animation_at_ms = Some(next_animation_deadline_from_clock(state, event.now_ms));
        return CursorTransitions::draw(mode, frame, true, RenderAllocationPolicy::ReuseOnly)
            .with_motion_class(motion_class)
            .with_next_animation_at_ms(next_animation_at_ms)
            .with_render_cleanup_action(RenderCleanupAction::Invalidate);
    }

    match source {
        EventSource::External => {
            let should_schedule_next_animation = state.is_animating() || state.is_settling();
            let next_animation_at_ms = if state.is_settling() {
                next_animation_deadline_from_settling(state, event.now_ms)
            } else if state.is_animating() && should_schedule_next_animation {
                Some(next_animation_deadline_from_clock(state, event.now_ms))
            } else {
                None
            };
            let cleanup_action = if should_schedule_next_animation {
                RenderCleanupAction::Invalidate
            } else {
                // Surprising: a quiescent external noop can be the last lifecycle edge after
                // ingress. Keep cleanup intent alive here so `Hot` can still converge to
                // `Cooling` and `Cold` without waiting for unrelated future ingress.
                RenderCleanupAction::Schedule
            };
            CursorTransitions::noop(mode, allow_real_cursor_updates)
                .with_motion_class(motion_class)
                .with_schedule_next_animation(should_schedule_next_animation)
                .with_next_animation_at_ms(next_animation_at_ms)
                .with_cursor_visibility(if state.is_animating() {
                    CursorVisibilityEffect::Keep
                } else {
                    CursorVisibilityEffect::Show
                })
                .with_render_cleanup_action(cleanup_action)
        }
        EventSource::AnimationTick => {
            let current_corners = state.current_corners();
            let target_corners = state.target_corners();
            let render_corners =
                corners_for_render(&state.config, &current_corners, &target_corners);
            let frame = build_render_frame(
                state,
                mode,
                render_corners,
                Vec::new(),
                0,
                target_position,
                vertical_bar,
            );
            CursorTransitions::draw(mode, frame, false, RenderAllocationPolicy::ReuseOnly)
                .with_motion_class(motion_class)
                .with_render_cleanup_action(RenderCleanupAction::NoAction)
        }
    }
}

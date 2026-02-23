use crate::animation::{compute_stiffnesses, corners_for_render, reached_target, simulate_step};
use crate::config::RuntimeConfig;
use crate::draw::{GradientInfo, RenderFrame};
use crate::state::{CursorLocation, CursorShape, RuntimeState};
use crate::types::{BASE_TIME_INTERVAL, EPSILON, Particle, Point, StepInput};
use nvim_utils::mode::{
    is_cmdline_mode, is_insert_like_mode, is_replace_like_mode, is_terminal_like_mode,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct CursorEventContext {
    pub(crate) row: f64,
    pub(crate) col: f64,
    pub(crate) now_ms: f64,
    pub(crate) seed: u32,
    pub(crate) cursor_location: CursorLocation,
    pub(crate) scroll_shift: Option<ScrollShift>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScrollShift {
    pub(crate) shift: f64,
    pub(crate) min_row: f64,
    pub(crate) max_row: f64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum EventSource {
    External,
    AnimationTick,
}

#[derive(Debug)]
pub(crate) enum RenderAction {
    Draw(Box<RenderFrame>),
    ClearAll,
    Noop,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum RenderCleanupAction {
    None,
    Schedule,
    Invalidate,
}

#[derive(Debug)]
pub(crate) struct RenderDecision {
    pub(crate) render_action: RenderAction,
    pub(crate) render_cleanup_action: RenderCleanupAction,
}

#[derive(Debug)]
pub(crate) struct CursorTransition {
    pub(crate) render_decision: RenderDecision,
    pub(crate) notify_delay_disabled: bool,
    pub(crate) command: Option<CursorCommand>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CursorCommand {
    StepIntervalMs(f64),
}

impl CursorTransition {
    fn with_render_action(render_action: RenderAction) -> Self {
        Self {
            render_decision: RenderDecision {
                render_action,
                render_cleanup_action: RenderCleanupAction::None,
            },
            notify_delay_disabled: false,
            command: None,
        }
    }

    fn with_command(mut self, command: CursorCommand) -> Self {
        self.command = Some(command);
        self
    }

    fn with_delay_notification(mut self, enabled: bool) -> Self {
        if enabled {
            self.notify_delay_disabled = true;
        }
        self
    }

    fn with_render_cleanup_action(mut self, action: RenderCleanupAction) -> Self {
        self.render_decision.render_cleanup_action = action;
        self
    }
}

struct CursorTransitions;

impl CursorTransitions {
    fn clear_all() -> CursorTransition {
        CursorTransition::with_render_action(RenderAction::ClearAll)
    }

    fn draw(frame: RenderFrame, step_interval_ms: Option<f64>) -> CursorTransition {
        let transition = CursorTransition::with_render_action(RenderAction::Draw(Box::new(frame)));
        match step_interval_ms {
            Some(value) => transition.with_command(CursorCommand::StepIntervalMs(value)),
            None => transition,
        }
    }

    fn noop() -> CursorTransition {
        CursorTransition::with_render_action(RenderAction::Noop)
    }

    fn noop_with_step(step_interval_ms: f64) -> CursorTransition {
        CursorTransition::with_render_action(RenderAction::Noop)
            .with_command(CursorCommand::StepIntervalMs(step_interval_ms))
    }
}

fn build_step_input(
    state: &RuntimeState,
    mode: &str,
    time_interval: f64,
    vertical_bar: bool,
    horizontal_bar: bool,
    particles: Vec<Particle>,
) -> StepInput {
    StepInput {
        mode: mode.to_string(),
        time_interval,
        config_time_interval: state.config.time_interval,
        current_corners: state.current_corners(),
        target_corners: state.target_corners(),
        velocity_corners: state.velocity_corners(),
        stiffnesses: state.stiffnesses(),
        max_length: state.config.max_length,
        max_length_insert_mode: state.config.max_length_insert_mode,
        damping: state.config.damping,
        damping_insert_mode: state.config.damping_insert_mode,
        delay_disable: state.config.delay_disable,
        particles,
        previous_center: state.previous_center(),
        particle_damping: state.config.particle_damping,
        particles_enabled: state.config.particles_enabled,
        particle_gravity: state.config.particle_gravity,
        particle_random_velocity: state.config.particle_random_velocity,
        particle_max_num: state.config.particle_max_num,
        particle_spread: state.config.particle_spread,
        particles_per_second: state.config.particles_per_second,
        particles_per_length: state.config.particles_per_length,
        particle_max_initial_velocity: state.config.particle_max_initial_velocity,
        particle_velocity_from_cursor: state.config.particle_velocity_from_cursor,
        particle_max_lifetime: state.config.particle_max_lifetime,
        particle_lifetime_distribution_exponent: state
            .config
            .particle_lifetime_distribution_exponent,
        min_distance_emit_particles: state.config.min_distance_emit_particles,
        vertical_bar,
        horizontal_bar,
        block_aspect_ratio: state.config.block_aspect_ratio,
        rng_state: state.rng_state(),
    }
}

pub(crate) fn build_render_frame(
    state: &RuntimeState,
    mode: &str,
    render_corners: [Point; 4],
    target: Point,
    vertical_bar: bool,
    gradient_indexes: Option<(usize, usize)>,
) -> RenderFrame {
    let gradient = gradient_indexes.map(|(index_head, index_tail)| {
        let origin = render_corners[index_head];
        let direction = Point {
            row: render_corners[index_tail].row - origin.row,
            col: render_corners[index_tail].col - origin.col,
        };
        let length_squared = direction.row * direction.row + direction.col * direction.col;
        let direction_scaled = if length_squared > 1.0 {
            Point {
                row: direction.row / length_squared,
                col: direction.col / length_squared,
            }
        } else {
            Point::ZERO
        };

        GradientInfo {
            origin,
            direction_scaled,
        }
    });

    RenderFrame {
        mode: mode.to_string(),
        corners: render_corners,
        target,
        target_corners: state.target_corners(),
        vertical_bar,
        particles: state.particles().to_vec(),
        cursor_color: state.config.cursor_color.clone(),
        cursor_color_insert_mode: state.config.cursor_color_insert_mode.clone(),
        normal_bg: state.config.normal_bg.clone(),
        transparent_bg_fallback_color: state.config.transparent_bg_fallback_color.clone(),
        cterm_cursor_colors: state.config.cterm_cursor_colors.clone(),
        cterm_bg: state.config.cterm_bg,
        color_at_cursor: state.color_at_cursor().map(str::to_owned),
        hide_target_hack: state.config.hide_target_hack,
        max_kept_windows: state.config.max_kept_windows,
        never_draw_over_target: state.config.never_draw_over_target,
        legacy_computing_symbols_support: state.config.legacy_computing_symbols_support,
        legacy_computing_symbols_support_vertical_bars: state
            .config
            .legacy_computing_symbols_support_vertical_bars,
        use_diagonal_blocks: state.config.use_diagonal_blocks,
        max_slope_horizontal: state.config.max_slope_horizontal,
        min_slope_vertical: state.config.min_slope_vertical,
        max_angle_difference_diagonal: state.config.max_angle_difference_diagonal,
        max_offset_diagonal: state.config.max_offset_diagonal,
        min_shade_no_diagonal: state.config.min_shade_no_diagonal,
        min_shade_no_diagonal_vertical_bar: state.config.min_shade_no_diagonal_vertical_bar,
        max_shade_no_matrix: state.config.max_shade_no_matrix,
        particle_max_lifetime: state.config.particle_max_lifetime,
        particle_switch_octant_braille: state.config.particle_switch_octant_braille,
        particles_over_text: state.config.particles_over_text,
        color_levels: state.config.color_levels,
        gamma: state.config.gamma,
        gradient_exponent: state.config.gradient_exponent,
        matrix_pixel_threshold: state.config.matrix_pixel_threshold,
        matrix_pixel_threshold_vertical_bar: state.config.matrix_pixel_threshold_vertical_bar,
        matrix_pixel_min_factor: state.config.matrix_pixel_min_factor,
        windows_zindex: state.config.windows_zindex,
        gradient,
    }
}

fn gradient_indexes_for_corners(
    current_corners: &[Point; 4],
    target_corners: &[Point; 4],
) -> Option<(usize, usize)> {
    let mut distance_head_to_target_squared = f64::INFINITY;
    let mut distance_tail_to_target_squared = 0.0_f64;
    let mut index_head = 0_usize;
    let mut index_tail = 0_usize;

    for index in 0..4 {
        let distance_squared = current_corners[index].distance_squared(target_corners[index]);
        if distance_squared < distance_head_to_target_squared {
            distance_head_to_target_squared = distance_squared;
            index_head = index;
        }
        if distance_squared > distance_tail_to_target_squared {
            distance_tail_to_target_squared = distance_squared;
            index_tail = index;
        }
    }

    if distance_tail_to_target_squared <= EPSILON {
        None
    } else {
        Some((index_head, index_tail))
    }
}

fn reset_animation_timing(state: &mut RuntimeState) {
    state.reset_animation_timing();
}

fn clamp_row_to_window(row: f64, scroll_shift: ScrollShift) -> f64 {
    row.max(scroll_shift.min_row).min(scroll_shift.max_row)
}

fn apply_scroll_shift_to_state(
    state: &mut RuntimeState,
    vertical_bar: bool,
    horizontal_bar: bool,
    scroll_shift: ScrollShift,
) {
    state.apply_scroll_shift(
        scroll_shift.shift,
        scroll_shift.min_row,
        scroll_shift.max_row,
        vertical_bar,
        horizontal_bar,
    );
}

fn should_jump_to_target(
    state: &RuntimeState,
    window_changed: bool,
    buffer_changed: bool,
    target_row: f64,
    target_col: f64,
) -> bool {
    if window_changed || buffer_changed {
        return !state.config.smear_between_buffers;
    }

    let current_corners = state.current_corners();
    let current_row = current_corners[0].row;
    let current_col = current_corners[0].col;
    let delta_row = (target_row - current_row).abs();
    let delta_col = (target_col - current_col).abs();

    (!state.config.smear_between_neighbor_lines && delta_row <= 1.5)
        || (delta_row < state.config.min_vertical_distance_smear
            && delta_col < state.config.min_horizontal_distance_smear)
        || (!state.config.smear_horizontally && delta_row <= 0.5)
        || (!state.config.smear_vertically && delta_col <= 0.5)
        || (!state.config.smear_diagonally && delta_row > 0.5 && delta_col > 0.5)
}

fn external_mode_ignores_cursor(config: &RuntimeConfig, mode: &str) -> bool {
    is_cmdline_mode(mode) && !config.smear_to_cmd
}

fn external_mode_requires_jump(config: &RuntimeConfig, mode: &str) -> bool {
    (is_insert_like_mode(mode) && !config.smear_insert_mode)
        || (is_replace_like_mode(mode) && !config.smear_replace_mode)
        || (is_terminal_like_mode(mode) && !config.smear_terminal_mode)
}

pub(crate) fn reduce_cursor_event(
    state: &mut RuntimeState,
    mode: &str,
    event: CursorEventContext,
    source: EventSource,
) -> CursorTransition {
    if !state.is_enabled() {
        state.stop_animation();
        reset_animation_timing(state);
        return CursorTransitions::clear_all()
            .with_render_cleanup_action(RenderCleanupAction::Invalidate);
    }

    if state.is_delay_disabled() {
        match source {
            EventSource::External => {
                return CursorTransitions::noop()
                    .with_render_cleanup_action(RenderCleanupAction::Schedule);
            }
            EventSource::AnimationTick if !state.is_animating() => {
                reset_animation_timing(state);
                return CursorTransitions::clear_all()
                    .with_render_cleanup_action(RenderCleanupAction::Schedule);
            }
            EventSource::AnimationTick => {}
        }
    }

    let vertical_bar = state.config.cursor_is_vertical_bar(mode);
    let horizontal_bar = state.config.cursor_is_horizontal_bar(mode);
    let cursor_shape = CursorShape::new(vertical_bar, horizontal_bar);
    let mut target_position = match source {
        EventSource::AnimationTick => state.target_position(),
        EventSource::External => Point {
            row: event.row,
            col: event.col,
        },
    };
    let (window_changed, buffer_changed) =
        state.tracked_location().map_or((false, false), |tracked| {
            (
                tracked.window_handle != event.cursor_location.window_handle,
                tracked.buffer_handle != event.cursor_location.buffer_handle,
            )
        });

    match source {
        EventSource::External => {
            if external_mode_ignores_cursor(&state.config, mode) {
                return CursorTransitions::noop()
                    .with_render_cleanup_action(RenderCleanupAction::Schedule);
            }
            if external_mode_requires_jump(&state.config, mode) {
                // Match upstream Lua behavior: jump updates position/target but does not
                // force-stop an in-flight animation loop or clear velocity state.
                state.jump_preserving_motion(target_position, cursor_shape, event.cursor_location);
                return CursorTransitions::clear_all()
                    .with_render_cleanup_action(RenderCleanupAction::Schedule);
            }
        }
        EventSource::AnimationTick => {}
    }

    if !state.is_initialized() {
        state.initialize_cursor(
            target_position,
            cursor_shape,
            event.seed,
            event.cursor_location,
        );
        let frame = build_render_frame(
            state,
            mode,
            state.current_corners(),
            target_position,
            vertical_bar,
            None,
        );
        return CursorTransitions::draw(frame, None)
            .with_render_cleanup_action(RenderCleanupAction::Schedule);
    }

    match source {
        EventSource::AnimationTick if !state.is_animating() => {
            return CursorTransitions::noop();
        }
        EventSource::AnimationTick | EventSource::External => {}
    }

    match source {
        EventSource::External => {
            if let Some(scroll_shift) = event.scroll_shift {
                apply_scroll_shift_to_state(state, vertical_bar, horizontal_bar, scroll_shift);
                target_position.row =
                    clamp_row_to_window(target_position.row - scroll_shift.shift, scroll_shift);
            }

            if should_jump_to_target(
                state,
                window_changed,
                buffer_changed,
                target_position.row,
                target_position.col,
            ) {
                state.jump_and_stop_animation(target_position, cursor_shape, event.cursor_location);
                return CursorTransitions::clear_all()
                    .with_render_cleanup_action(RenderCleanupAction::Schedule);
            }
            state.set_target(target_position, cursor_shape);
            let current_corners = state.current_corners();
            let target_corners = state.target_corners();
            let stiffnesses =
                compute_stiffnesses(&state.config, mode, &current_corners, &target_corners);
            state.set_stiffnesses(stiffnesses);
        }
        EventSource::AnimationTick => {}
    }

    let was_animating = state.is_animating();
    let just_started = match source {
        EventSource::External => {
            if !was_animating {
                state.start_animation_towards_target();
            }
            state.update_tracking(event.cursor_location);
            !was_animating && state.is_animating()
        }
        EventSource::AnimationTick => false,
    };

    let should_advance = match source {
        EventSource::AnimationTick => state.is_animating(),
        // Match upstream behavior: target updates do not advance physics while already animating.
        // The first transition after a jump starts animation immediately.
        EventSource::External => just_started,
    };

    if should_advance {
        let step_interval = if just_started {
            state.set_last_tick_ms(Some(event.now_ms));
            BASE_TIME_INTERVAL
        } else {
            let interval = state.last_tick_ms().map_or(BASE_TIME_INTERVAL, |previous| {
                (event.now_ms - previous).max(0.0)
            });
            state.set_last_tick_ms(Some(event.now_ms));
            interval
        };

        let particles = state.take_particles();
        let step_input = build_step_input(
            state,
            mode,
            step_interval,
            vertical_bar,
            horizontal_bar,
            particles,
        );
        let step_output = simulate_step(step_input);
        let step_indexes = (step_output.index_head, step_output.index_tail);
        let disabled_due_to_delay = step_output.disabled_due_to_delay;
        state.apply_step_output(step_output);

        let notify_delay_disabled = if disabled_due_to_delay && !state.is_delay_disabled() {
            state.set_delay_disabled(true);
            true
        } else {
            false
        };

        let current_corners = state.current_corners();
        let target_corners = state.target_corners();
        let velocity_corners = state.velocity_corners();
        if reached_target(
            &state.config,
            mode,
            &current_corners,
            &target_corners,
            &velocity_corners,
            state.particles(),
        ) {
            state.settle_at_target();
            state.stop_animation();
            reset_animation_timing(state);
            return CursorTransitions::clear_all()
                .with_delay_notification(notify_delay_disabled)
                .with_render_cleanup_action(RenderCleanupAction::Schedule);
        }

        if state.lag_ms() > EPSILON {
            return CursorTransitions::noop_with_step(step_interval)
                .with_delay_notification(notify_delay_disabled)
                .with_render_cleanup_action(RenderCleanupAction::Invalidate);
        }

        let current_corners = state.current_corners();
        let target_corners = state.target_corners();
        let render_corners = corners_for_render(&state.config, &current_corners, &target_corners);
        let frame = build_render_frame(
            state,
            mode,
            render_corners,
            target_position,
            vertical_bar,
            Some(step_indexes),
        );
        return CursorTransitions::draw(frame, Some(step_interval))
            .with_delay_notification(notify_delay_disabled)
            .with_render_cleanup_action(RenderCleanupAction::Invalidate);
    }

    match source {
        EventSource::External => {
            CursorTransitions::noop().with_render_cleanup_action(RenderCleanupAction::Invalidate)
        }
        EventSource::AnimationTick => {
            let current_corners = state.current_corners();
            let target_corners = state.target_corners();
            let gradient_indexes = gradient_indexes_for_corners(&current_corners, &target_corners);
            let render_corners =
                corners_for_render(&state.config, &current_corners, &target_corners);
            let frame = build_render_frame(
                state,
                mode,
                render_corners,
                target_position,
                vertical_bar,
                gradient_indexes,
            );
            CursorTransitions::draw(frame, None)
                .with_render_cleanup_action(RenderCleanupAction::None)
        }
    }
}

pub(crate) fn as_delay_ms(value: f64) -> u64 {
    let clamped = if value.is_finite() {
        value.max(0.0).floor()
    } else {
        0.0
    };
    if clamped > u64::MAX as f64 {
        u64::MAX
    } else {
        clamped as u64
    }
}

pub(crate) fn next_animation_delay_ms(
    state: &mut RuntimeState,
    step_interval_ms: f64,
    callback_duration_ms: f64,
) -> u64 {
    let lag_ms = (state.lag_ms() + step_interval_ms - state.config.time_interval).max(0.0);
    state.set_lag_ms(lag_ms);

    let mut delay_ms = (state.config.time_interval - callback_duration_ms).max(0.0);
    if state.lag_ms() <= delay_ms {
        delay_ms -= state.lag_ms();
        state.set_lag_ms(0.0);
    } else {
        state.set_lag_ms(state.lag_ms() - delay_ms);
        delay_ms = 0.0;
    }

    as_delay_ms(delay_ms)
}

pub(crate) fn external_settle_delay_ms(delay_event_to_smear: f64) -> u64 {
    as_delay_ms(delay_event_to_smear)
}

#[cfg(test)]
mod tests {
    use super::{
        CursorEventContext, CursorTransition, EventSource, RenderAction, RenderCleanupAction,
        reduce_cursor_event,
    };
    use crate::state::{CursorLocation, RuntimeState};
    use proptest::collection::vec;
    use proptest::prelude::*;

    fn render_action(transition: &CursorTransition) -> &RenderAction {
        &transition.render_decision.render_action
    }

    fn render_cleanup_action(transition: &CursorTransition) -> RenderCleanupAction {
        transition.render_decision.render_cleanup_action
    }

    fn event(row: f64, col: f64) -> CursorEventContext {
        CursorEventContext {
            row,
            col,
            now_ms: 100.0,
            seed: 7,
            cursor_location: CursorLocation::new(10, 20, 1, 1),
            scroll_shift: None,
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
        }
    }

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
    fn first_external_event_initializes_and_draws() {
        let mut state = RuntimeState::default();
        let effects = reduce_cursor_event(&mut state, "n", event(5.0, 6.0), EventSource::External);
        assert!(matches!(render_action(&effects), RenderAction::Draw(_)));
        assert_eq!(
            render_cleanup_action(&effects),
            RenderCleanupAction::Schedule
        );
        assert!(state.is_initialized());
        assert!(!state.is_animating());
    }

    #[test]
    fn animation_tick_is_noop_when_not_running() {
        let mut state = RuntimeState::default();
        state.mark_initialized();
        state.stop_animation();
        let effects =
            reduce_cursor_event(&mut state, "n", event(5.0, 6.0), EventSource::AnimationTick);
        assert!(matches!(render_action(&effects), RenderAction::Noop));
        assert_eq!(render_cleanup_action(&effects), RenderCleanupAction::None);
    }

    #[test]
    fn window_switch_smears_when_smear_between_buffers_enabled() {
        let mut state = RuntimeState::default();
        state.config.smear_between_buffers = true;

        let _ = reduce_cursor_event(&mut state, "n", event(5.0, 6.0), EventSource::External);
        let switched_window = CursorEventContext {
            row: 20.0,
            col: 30.0,
            now_ms: 120.0,
            seed: 99,
            cursor_location: CursorLocation::new(999, 20, 1, 1),
            scroll_shift: None,
        };

        let effects = reduce_cursor_event(&mut state, "n", switched_window, EventSource::External);
        assert!(matches!(render_action(&effects), RenderAction::Draw(_)));
        assert_eq!(
            render_cleanup_action(&effects),
            RenderCleanupAction::Invalidate
        );
    }

    #[test]
    fn window_switch_clears_when_smear_between_buffers_disabled() {
        let mut state = RuntimeState::default();
        state.config.smear_between_buffers = false;

        let _ = reduce_cursor_event(&mut state, "n", event(5.0, 6.0), EventSource::External);
        let switched_window = CursorEventContext {
            row: 20.0,
            col: 30.0,
            now_ms: 120.0,
            seed: 99,
            cursor_location: CursorLocation::new(999, 20, 1, 1),
            scroll_shift: None,
        };

        let effects = reduce_cursor_event(&mut state, "n", switched_window, EventSource::External);
        assert!(matches!(render_action(&effects), RenderAction::ClearAll));
        assert_eq!(
            render_cleanup_action(&effects),
            RenderCleanupAction::Schedule
        );
    }

    #[test]
    fn repeated_window_switches_keep_smearing_when_enabled() {
        let mut state = RuntimeState::default();
        state.config.smear_between_buffers = true;

        let _ = reduce_cursor_event(&mut state, "n", event(5.0, 6.0), EventSource::External);

        let mut now_ms = 120.0;
        let mut observed_draw = false;
        for step in 0_u32..64 {
            let window_handle = if step % 2 == 0 { 100_i64 } else { 101_i64 };
            let event = CursorEventContext {
                row: 20.0 + f64::from(step) * 0.5,
                col: 30.0 + f64::from(step) * 0.5,
                now_ms,
                seed: 1000 + step,
                cursor_location: CursorLocation::new(window_handle, 20, 1, 1),
                scroll_shift: None,
            };
            let effects = reduce_cursor_event(&mut state, "n", event, EventSource::External);
            if matches!(render_action(&effects), RenderAction::Draw(_)) {
                observed_draw = true;
            }
            assert!(
                !matches!(render_action(&effects), RenderAction::ClearAll),
                "unexpected clear-all for repeated window switch step {step}"
            );
            assert_ne!(
                render_cleanup_action(&effects),
                RenderCleanupAction::Schedule,
                "unexpected cleanup scheduling while repeatedly switching windows at step {step}"
            );
            now_ms += 16.0;
        }
        assert!(observed_draw, "expected repeated switches to produce draws");
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(96))]

        #[test]
        fn prop_window_change_clears_when_smear_between_buffers_disabled(
            start_row in 1_u16..120,
            start_col in 1_u16..220,
            switched_row in 120_u16..280,
            switched_col in 120_u16..280,
            initial_window in any::<i64>(),
            initial_buffer in any::<i64>(),
            window_delta in 1_i16..32,
            buffer_delta in 1_i16..32,
            initial_seed in any::<u32>(),
            switched_seed in any::<u32>(),
        ) {
            let mut state = RuntimeState::default();
            state.config.smear_between_buffers = false;

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
                initial_buffer.wrapping_add(i64::from(buffer_delta)),
            );
            let effects = reduce_cursor_event(&mut state, "n", switched, EventSource::External);
            prop_assert!(matches!(render_action(&effects), RenderAction::ClearAll));
            prop_assert_eq!(
                render_cleanup_action(&effects),
                RenderCleanupAction::Schedule
            );
        }

        #[test]
        fn prop_repeated_window_switches_do_not_clear_when_smear_between_buffers_enabled(
            switches in vec((20_u16..260, 20_u16..260, any::<u32>()), 1..96),
        ) {
            let mut state = RuntimeState::default();
            state.config.smear_between_buffers = true;

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
                let effects = reduce_cursor_event(&mut state, "n", step_event, EventSource::External);
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
            let lag_before = state.lag_ms();

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
                    RenderCleanupAction::None
                );
                prop_assert!(state.is_initialized());
                prop_assert!(!state.is_animating());
                prop_assert_eq!(state.tracked_location(), tracked_before);
                prop_assert_eq!(state.last_tick_ms(), last_tick_before);
                prop_assert_eq!(state.lag_ms(), lag_before);
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
                        RenderCleanupAction::None
                    );
                }
                if source == EventSource::External
                    && matches!(render_action(&effects), RenderAction::Noop)
                {
                    prop_assert_ne!(
                        render_cleanup_action(&effects),
                        RenderCleanupAction::None
                    );
                }

                now_ms += 8.0;
            }
        }
    }
}

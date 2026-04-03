use super::ScrollShift;
use super::as_delay_ms;
use crate::state::RuntimeState;
use crate::types::Particle;
use crate::types::Point;
use crate::types::RenderFrame;
use crate::types::RenderStepSample;
use crate::types::StepInput;

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
        config_time_interval: time_interval,
        head_response_ms: state.config.head_response_ms,
        damping_ratio: state.config.damping_ratio,
        current_corners: state.current_corners(),
        trail_origin_corners: state.trail_origin_corners(),
        target_corners: state.target_corners(),
        spring_velocity_corners: state.spring_velocity_corners(),
        trail_elapsed_ms: state.trail_elapsed_ms(),
        max_length: state.config.max_length,
        max_length_insert_mode: state.config.max_length_insert_mode,
        trail_duration_ms: state.config.trail_duration_ms,
        trail_min_distance: state.config.trail_min_distance,
        trail_thickness: state.config.trail_thickness,
        trail_thickness_x: state.config.trail_thickness_x,
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
    step_samples: Vec<RenderStepSample>,
    planner_idle_steps: u32,
    target: Point,
    vertical_bar: bool,
) -> RenderFrame {
    RenderFrame {
        mode: mode.to_string(),
        corners: render_corners,
        step_samples: std::sync::Arc::from(step_samples),
        planner_idle_steps,
        target,
        target_corners: state.target_corners(),
        vertical_bar,
        trail_stroke_id: state.trail_stroke_id(),
        retarget_epoch: state.retarget_epoch(),
        particles: std::sync::Arc::from(state.particles().to_vec()),
        color_at_cursor: state.color_at_cursor(),
        static_config: state.render_static_config(),
    }
}

pub(super) fn reset_animation_timing(state: &mut RuntimeState) {
    state.reset_animation_timing();
}

pub(super) fn next_animation_deadline_from_settling(
    state: &RuntimeState,
    now_ms: f64,
) -> Option<u64> {
    state
        .settle_deadline_ms()
        .map(|deadline| as_delay_ms(deadline.max(now_ms + 1.0)))
}

pub(super) fn next_animation_deadline_from_clock(state: &mut RuntimeState, now_ms: f64) -> u64 {
    let next_frame_at_ms = state.advance_next_frame_deadline(now_ms);
    as_delay_ms(next_frame_at_ms.max(now_ms + 1.0))
}

pub(super) fn clamp_row_to_window(row: f64, scroll_shift: ScrollShift) -> f64 {
    row.max(scroll_shift.min_row).min(scroll_shift.max_row)
}

pub(super) fn apply_scroll_shift_to_state(
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

pub(super) fn step_input(
    state: &RuntimeState,
    mode: &str,
    time_interval: f64,
    vertical_bar: bool,
    horizontal_bar: bool,
    particles: Vec<Particle>,
) -> StepInput {
    build_step_input(
        state,
        mode,
        time_interval,
        vertical_bar,
        horizontal_bar,
        particles,
    )
}

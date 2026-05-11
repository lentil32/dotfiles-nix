use super::*;
use pretty_assertions::assert_eq;

fn retain_particle_cold_storage(state: &mut RuntimeState) {
    state.apply_step_output(StepOutput {
        current_corners: state.current_corners(),
        velocity_corners: state.velocity_corners(),
        spring_velocity_corners: state.spring_velocity_corners(),
        trail_elapsed_ms: state.trail_elapsed_ms(),
        particles: vec![Particle {
            position: RenderPoint { row: 5.0, col: 8.0 },
            velocity: RenderPoint { row: 1.0, col: 2.0 },
            lifetime: 10_000.0,
        }],
        previous_center: state.previous_center(),
        index_head: 0,
        index_tail: 3,
        rng_state: state.rng_state(),
    });
    let _ = state.shared_particle_screen_cells();
    state.reclaim_preview_particles_scratch(Vec::with_capacity(8));
    let mut scratch = state.take_render_step_samples_scratch();
    scratch.reserve(4);
    scratch.push(RenderStepSample::new(state.current_corners(), 16.0));
    state.reclaim_render_step_samples_scratch(scratch);
}

fn assert_particle_cold_storage_retained(state: &RuntimeState) {
    assert!(state.preview_particles_scratch_capacity() > 0);
    assert!(state.render_step_samples_scratch_capacity() > 0);
    assert!(state.particle_aggregation_scratch_index_capacity() > 0);
    assert!(state.particle_aggregation_scratch_cells_capacity() > 0);
    assert!(state.particle_aggregation_scratch_screen_cells_capacity() > 0);
    assert!(state.has_cached_aggregated_particle_cells());
    assert!(state.has_cached_particle_screen_cells());
}

fn assert_particle_cold_storage_released(state: &RuntimeState) {
    assert_eq!(state.preview_particles_scratch_capacity(), 0);
    assert_eq!(state.render_step_samples_scratch_capacity(), 0);
    assert_eq!(state.particle_aggregation_scratch_index_capacity(), 0);
    assert_eq!(state.particle_aggregation_scratch_cells_capacity(), 0);
    assert_eq!(
        state.particle_aggregation_scratch_screen_cells_capacity(),
        0
    );
    assert!(!state.has_cached_aggregated_particle_cells());
    assert!(!state.has_cached_particle_screen_cells());
}

#[test]
fn long_gap_while_animating_clears_stale_motion_and_stops_animation() {
    let mut state = animating_runtime_towards_target(|state| {
        state.config.time_interval = 16.0;
        state.config.simulation_hz = 120.0;
        state.config.max_simulation_steps_per_frame = 16;
    });
    retain_particle_cold_storage(&mut state);
    assert_particle_cold_storage_retained(&state);
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
                cursor_visibility: CursorVisibilityEffect::Show,
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
    assert_particle_cold_storage_released(&state);
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
    retain_particle_cold_storage(&mut state);
    assert_particle_cold_storage_retained(&state);
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
                cursor_visibility: CursorVisibilityEffect::Show,
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
    assert_particle_cold_storage_released(&state);
    assert!(!state.is_animating());
    assert!(!state.is_draining());
}

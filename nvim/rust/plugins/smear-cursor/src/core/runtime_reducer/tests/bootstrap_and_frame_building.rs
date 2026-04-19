use super::*;
use crate::core::runtime_reducer::reduce_cursor_event_for_perf_class;
use crate::core::state::BufferPerfClass;
use pretty_assertions::assert_eq;

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
fn first_external_event_bootstraps_frame_cleanup_allocation_and_idle_state() {
    let (state, transition) = initialized_runtime("n", |_| {});

    assert!(matches!(render_action(&transition), RenderAction::Draw(_)));
    assert_eq!(
        render_cleanup_action(&transition),
        RenderCleanupAction::Schedule
    );
    assert_eq!(
        render_allocation_policy(&transition),
        RenderAllocationPolicy::BootstrapIfPoolEmpty
    );
    assert!(state.is_initialized());
    assert!(!state.is_animating());
}

#[test]
fn build_render_frame_preserves_core_cursor_geometry() {
    let mut state = RuntimeState::default();
    let location = TrackedCursor::fixture(10, 20, 1, 1);
    let shape = CursorShape::block();
    let position = RenderPoint { row: 5.0, col: 7.0 };
    state.initialize_cursor(position, shape, 3, &location);
    let current_corners = state.current_corners();
    let target_position = state.target_position();

    let frame = crate::core::runtime_reducer::build_render_frame(
        &mut state,
        crate::core::runtime_reducer::RenderFrameRequest {
            mode: "n",
            render_corners: current_corners,
            step_samples: Vec::new(),
            planner_idle_steps: 0,
            target: target_position,
            vertical_bar: false,
            buffer_perf_class: BufferPerfClass::Full,
        },
    );

    assert_eq!(frame.corners, state.current_corners());
    assert!(frame.step_samples.is_empty());
    assert_eq!(frame.target, state.target_position());
    assert_eq!(frame.target_corners, state.target_corners());
}

#[test]
fn build_render_frame_reclaims_render_step_sample_scratch() {
    let mut state = RuntimeState::default();
    let location = TrackedCursor::fixture(10, 20, 1, 1);
    let shape = CursorShape::block();
    let position = RenderPoint { row: 5.0, col: 7.0 };
    state.initialize_cursor(position, shape, 3, &location);
    let current_corners = state.current_corners();
    let target_position = state.target_position();
    let retained_sample = RenderStepSample::new(current_corners, 16.0);
    let mut step_samples = state.take_render_step_samples_scratch();
    step_samples.reserve(4);
    step_samples.push(retained_sample.clone());
    let scratch_capacity = step_samples.capacity();
    let scratch_ptr = step_samples.as_ptr();

    let frame = crate::core::runtime_reducer::build_render_frame(
        &mut state,
        crate::core::runtime_reducer::RenderFrameRequest {
            mode: "n",
            render_corners: current_corners,
            step_samples,
            planner_idle_steps: 0,
            target: target_position,
            vertical_bar: false,
            buffer_perf_class: BufferPerfClass::Full,
        },
    );

    assert_eq!(frame.step_samples.as_ref(), [retained_sample].as_slice());
    assert_eq!(
        state.render_step_samples_scratch_capacity(),
        scratch_capacity
    );
    assert_eq!(state.render_step_samples_scratch_ptr(), scratch_ptr);
}

#[test]
fn build_render_frame_omits_particles_when_perf_class_disables_ornamental_effects() {
    let mut state = RuntimeState::default();
    let location = TrackedCursor::fixture(10, 20, 1, 1);
    let shape = CursorShape::block();
    let position = RenderPoint { row: 5.0, col: 7.0 };
    state.initialize_cursor(position, shape, 3, &location);
    state.apply_step_output(StepOutput {
        current_corners: state.current_corners(),
        velocity_corners: state.velocity_corners(),
        spring_velocity_corners: state.spring_velocity_corners(),
        trail_elapsed_ms: state.trail_elapsed_ms(),
        particles: vec![Particle {
            position: RenderPoint {
                row: 5.25,
                col: 7.5,
            },
            velocity: RenderPoint::ZERO,
            lifetime: 1.0,
        }],
        previous_center: state.previous_center(),
        index_head: 0,
        index_tail: 0,
        rng_state: state.rng_state(),
    });
    let current_corners = state.current_corners();
    let target_position = state.target_position();

    let frame = crate::core::runtime_reducer::build_render_frame(
        &mut state,
        crate::core::runtime_reducer::RenderFrameRequest {
            mode: "n",
            render_corners: current_corners,
            step_samples: Vec::new(),
            planner_idle_steps: 0,
            target: target_position,
            vertical_bar: false,
            buffer_perf_class: BufferPerfClass::FastMotion,
        },
    );

    assert!(!frame.has_particles());
    assert!(frame.aggregated_particle_cells().is_empty());
}

#[test]
fn build_render_frame_reuses_cached_particle_aggregation() {
    let mut state = RuntimeState::default();
    let location = TrackedCursor::fixture(10, 20, 1, 1);
    let shape = CursorShape::block();
    let position = RenderPoint { row: 5.0, col: 7.0 };
    state.initialize_cursor(position, shape, 3, &location);
    state.apply_step_output(StepOutput {
        current_corners: state.current_corners(),
        velocity_corners: state.velocity_corners(),
        spring_velocity_corners: state.spring_velocity_corners(),
        trail_elapsed_ms: state.trail_elapsed_ms(),
        particles: vec![Particle {
            position: RenderPoint {
                row: 5.25,
                col: 7.5,
            },
            velocity: RenderPoint::ZERO,
            lifetime: 1.0,
        }],
        previous_center: state.previous_center(),
        index_head: 0,
        index_tail: 0,
        rng_state: state.rng_state(),
    });

    let cached_aggregates = state.shared_aggregated_particle_cells();
    let current_corners = state.current_corners();
    let target_position = state.target_position();
    let frame = crate::core::runtime_reducer::build_render_frame(
        &mut state,
        crate::core::runtime_reducer::RenderFrameRequest {
            mode: "n",
            render_corners: current_corners,
            step_samples: Vec::new(),
            planner_idle_steps: 0,
            target: target_position,
            vertical_bar: false,
            buffer_perf_class: BufferPerfClass::Full,
        },
    );

    assert!(std::sync::Arc::ptr_eq(
        &cached_aggregates,
        &frame.aggregated_particle_cells
    ));
}

#[test]
fn build_render_frame_reuses_cached_particle_screen_cells_for_background_probes() {
    let mut state = RuntimeState::default();
    state.config.particles_over_text = false;
    let location = TrackedCursor::fixture(10, 20, 1, 1);
    let shape = CursorShape::block();
    let position = RenderPoint { row: 5.0, col: 7.0 };
    state.initialize_cursor(position, shape, 3, &location);
    state.apply_step_output(StepOutput {
        current_corners: state.current_corners(),
        velocity_corners: state.velocity_corners(),
        spring_velocity_corners: state.spring_velocity_corners(),
        trail_elapsed_ms: state.trail_elapsed_ms(),
        particles: vec![Particle {
            position: RenderPoint {
                row: 5.25,
                col: 7.5,
            },
            velocity: RenderPoint::ZERO,
            lifetime: 1.0,
        }],
        previous_center: state.previous_center(),
        index_head: 0,
        index_tail: 0,
        rng_state: state.rng_state(),
    });

    let cached_screen_cells = state.shared_particle_screen_cells();
    let current_corners = state.current_corners();
    let target_position = state.target_position();
    let frame = crate::core::runtime_reducer::build_render_frame(
        &mut state,
        crate::core::runtime_reducer::RenderFrameRequest {
            mode: "n",
            render_corners: current_corners,
            step_samples: Vec::new(),
            planner_idle_steps: 0,
            target: target_position,
            vertical_bar: false,
            buffer_perf_class: BufferPerfClass::Full,
        },
    );

    assert!(std::sync::Arc::ptr_eq(
        &cached_screen_cells,
        &frame.particle_screen_cells
    ));
}

#[test]
fn build_render_frame_matches_with_warm_or_purged_particle_cache() {
    let mut warm_state = RuntimeState::default();
    warm_state.config.particles_over_text = false;
    let location = TrackedCursor::fixture(10, 20, 1, 1);
    let shape = CursorShape::block();
    let position = RenderPoint { row: 5.0, col: 7.0 };
    warm_state.initialize_cursor(position, shape, 3, &location);
    warm_state.apply_step_output(StepOutput {
        current_corners: warm_state.current_corners(),
        velocity_corners: warm_state.velocity_corners(),
        spring_velocity_corners: warm_state.spring_velocity_corners(),
        trail_elapsed_ms: warm_state.trail_elapsed_ms(),
        particles: vec![Particle {
            position: RenderPoint {
                row: 5.25,
                col: 7.5,
            },
            velocity: RenderPoint::ZERO,
            lifetime: 1.0,
        }],
        previous_center: warm_state.previous_center(),
        index_head: 0,
        index_tail: 0,
        rng_state: warm_state.rng_state(),
    });
    let current_corners = warm_state.current_corners();
    let target_position = warm_state.target_position();

    let _ = warm_state.shared_particle_screen_cells();
    let warm_frame = crate::core::runtime_reducer::build_render_frame(
        &mut warm_state,
        crate::core::runtime_reducer::RenderFrameRequest {
            mode: "n",
            render_corners: current_corners,
            step_samples: Vec::new(),
            planner_idle_steps: 0,
            target: target_position,
            vertical_bar: false,
            buffer_perf_class: BufferPerfClass::Full,
        },
    );

    let mut purged_state = warm_state.clone();
    purged_state.purge_cached_particle_artifacts();
    assert!(!purged_state.has_cached_aggregated_particle_cells());
    assert!(!purged_state.has_cached_particle_screen_cells());
    assert_eq!(purged_state.semantic_view(), warm_state.semantic_view());

    let purged_frame = crate::core::runtime_reducer::build_render_frame(
        &mut purged_state,
        crate::core::runtime_reducer::RenderFrameRequest {
            mode: "n",
            render_corners: current_corners,
            step_samples: Vec::new(),
            planner_idle_steps: 0,
            target: target_position,
            vertical_bar: false,
            buffer_perf_class: BufferPerfClass::Full,
        },
    );

    assert_eq!(purged_frame, warm_frame);
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

#[test]
fn degraded_perf_class_clears_particles_before_runtime_step_and_frame_export() {
    let mut state = RuntimeState::default();
    state.config.delay_event_to_smear = 0.0;
    state.config.time_interval = 16.0;
    state.config.simulation_hz = 240.0;
    state.config.max_simulation_steps_per_frame = 16;
    state.config.particles_enabled = true;

    let _ = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 6.0, 100.0),
        EventSource::External,
    );
    state.apply_step_output(StepOutput {
        current_corners: state.current_corners(),
        velocity_corners: state.velocity_corners(),
        spring_velocity_corners: state.spring_velocity_corners(),
        trail_elapsed_ms: state.trail_elapsed_ms(),
        particles: vec![Particle {
            position: RenderPoint {
                row: 5.25,
                col: 6.5,
            },
            velocity: RenderPoint::ZERO,
            lifetime: 1.0,
        }],
        previous_center: state.previous_center(),
        index_head: 0,
        index_tail: 0,
        rng_state: state.rng_state(),
    });

    let effects = reduce_cursor_event_for_perf_class(
        &mut state,
        "n",
        event_at(5.0, 16.0, 108.0),
        EventSource::External,
        BufferPerfClass::FastMotion,
    );
    let frame = draw_frame(&effects).expect("degraded retarget should still draw");

    assert!(state.particles().is_empty());
    assert!(!frame.has_particles());
    assert!(frame.aggregated_particle_cells().is_empty());
}

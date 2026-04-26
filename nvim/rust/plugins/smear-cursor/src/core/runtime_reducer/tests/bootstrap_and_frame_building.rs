use super::*;
use crate::core::runtime_reducer::reduce_cursor_event_for_perf_class;
use crate::core::state::BufferPerfClass;
use pretty_assertions::assert_eq;

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

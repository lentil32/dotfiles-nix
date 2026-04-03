use super::*;

#[test]
fn apply_step_output_replaces_the_mutable_simulation_snapshot() {
    let mut state = RuntimeState {
        target_corners: [point(11.0, 12.0); 4],
        trail_origin_corners: [point(13.0, 14.0); 4],
        ..RuntimeState::default()
    };
    state.transient.target_position = point(15.0, 16.0);
    state.set_color_at_cursor(Some(0x00AB_CDEF));
    let output = sample_step_output();
    let mut expected = state.clone();
    expected.current_corners = output.current_corners;
    expected.velocity_corners = output.velocity_corners;
    expected.spring_velocity_corners = output.spring_velocity_corners;
    expected.trail_elapsed_ms = output.trail_elapsed_ms;
    expected.previous_center = output.previous_center;
    expected.rng_state = output.rng_state;
    expected.particles = std::sync::Arc::new(output.particles.clone());

    state.apply_step_output(output);

    pretty_assertions::assert_eq!(state, expected);
}

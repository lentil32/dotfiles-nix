use super::*;

#[test]
fn prepared_motion_round_trips_runtime_dynamics_without_overwriting_config() {
    let tracked = location(11, 22, 33, 44);
    let mut source = RuntimeState::default();
    source.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    source.begin_settling(point(8.0, 9.0), default_shape(), &tracked, 100.0);
    source.set_color_at_cursor(Some(0x00AB_CDEF));
    source.set_last_mode_was_cmdline(true);
    source.set_last_tick_ms(Some(144.0));
    source.apply_step_output(sample_step_output());

    let mut target = RuntimeState::default();
    target.config.delay_event_to_smear = 27.0;
    target.config.hide_target_hack = false;
    target.refresh_render_static_config();
    target.set_enabled(false);

    let mut expected = target.clone();
    expected.animation_phase = source.animation_phase.clone();
    expected.current_corners = source.current_corners;
    expected.trail_origin_corners = source.trail_origin_corners;
    expected.target_corners = source.target_corners;
    expected.velocity_corners = source.velocity_corners;
    expected.spring_velocity_corners = source.spring_velocity_corners;
    expected.trail_elapsed_ms = source.trail_elapsed_ms;
    expected.particles = source.particles.clone();
    expected.previous_center = source.previous_center;
    expected.rng_state = source.rng_state;
    expected.transient = source.transient.clone();

    target.apply_prepared_motion(source.prepared_motion());

    pretty_assertions::assert_eq!(target, expected);
}

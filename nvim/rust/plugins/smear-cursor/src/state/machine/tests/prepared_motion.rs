use super::*;

#[test]
fn prepared_motion_round_trips_runtime_dynamics_without_overwriting_config() {
    let tracked = location(11, 22, 33, 44);
    let mut source = RuntimeState::default();
    source.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    replace_target_preserving_tracking(&mut source, point(8.0, 9.0), default_shape());
    source.start_animation_towards_target();
    source.set_color_at_cursor(Some(0x00AB_CDEF));
    source.record_observed_mode(/*current_is_cmdline*/ true);
    source.set_last_tick_ms(Some(144.0));
    source.apply_step_output(sample_step_output());

    let mut target = RuntimeState::default();
    target.config.delay_event_to_smear = 27.0;
    target.config.hide_target_hack = false;
    target.commit_runtime_config_update();
    target.set_enabled(false);

    let mut expected = target.clone();
    expected.animation_phase = source.animation_phase.clone();
    expected.current_corners = source.current_corners;
    expected.target = source.target.clone();
    expected.trail = source.trail.clone();
    expected.velocity_corners = source.velocity_corners;
    expected.spring_velocity_corners = source.spring_velocity_corners;
    expected.particles = source.particles.clone();
    expected.previous_center = source.previous_center;
    expected.rng_state = source.rng_state;
    expected.transient = source.transient.clone();

    target.apply_prepared_motion(source.prepared_motion());

    pretty_assertions::assert_eq!(target.semantic_view(), expected.semantic_view());
}

#[test]
fn planning_preview_mutation_does_not_write_back_into_source_runtime() {
    let tracked = location(11, 22, 33, 44);
    let mut source = RuntimeState::default();
    source.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    replace_target_preserving_tracking(&mut source, point(8.0, 9.0), default_shape());
    source.start_animation_towards_target();
    source.set_color_at_cursor(Some(0x00AB_CDEF));
    source.record_observed_mode(/*current_is_cmdline*/ true);
    source.apply_step_output(sample_step_output());
    let expected = source.clone();

    let mut preview = RuntimePreview::new(&mut source);
    let preview_runtime = preview.runtime_mut();
    preview_runtime.set_enabled(false);
    replace_target_preserving_tracking(preview_runtime, point(21.0, 34.0), default_shape());
    preview_runtime.set_color_at_cursor(Some(0x0012_3456));
    preview_runtime.record_observed_mode(/*current_is_cmdline*/ false);
    preview_runtime.start_animation_towards_target();

    pretty_assertions::assert_eq!(source, expected);
}

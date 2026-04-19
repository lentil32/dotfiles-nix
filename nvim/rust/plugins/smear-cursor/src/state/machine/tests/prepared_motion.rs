use super::*;
use std::sync::Arc;

#[test]
fn prepared_motion_round_trips_runtime_dynamics_without_overwriting_config() {
    let tracked = location(11, 22, 33, 44);
    let mut source = RuntimeState::default();
    source.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    source.set_target(point(8.0, 9.0), default_shape());
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
fn prepared_motion_restores_particle_caches_without_reinvalidating_them() {
    let tracked = location(11, 22, 33, 44);
    let mut source = RuntimeState::default();
    source.config.particles_over_text = false;
    source.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    source.apply_step_output(sample_step_output());

    let cached_aggregates = source.shared_aggregated_particle_cells();
    let cached_screen_cells = source.shared_particle_screen_cells();
    let prepared_motion = source.prepared_motion();

    let mut target = RuntimeState::default();
    target.apply_prepared_motion(prepared_motion);

    assert!(target.has_cached_aggregated_particle_cells());
    assert!(target.has_cached_particle_screen_cells());
    assert!(Arc::ptr_eq(
        &cached_aggregates,
        &target.shared_aggregated_particle_cells()
    ));
    assert!(Arc::ptr_eq(
        &cached_screen_cells,
        &target.shared_particle_screen_cells()
    ));
}

#[test]
fn prepared_motion_does_not_alias_live_particle_storage() {
    let tracked = location(11, 22, 33, 44);
    let mut source = RuntimeState::default();
    source.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    source.apply_step_output(sample_step_output());

    let prepared_motion = source.prepared_motion();
    let expected_particles = source.particles.clone();
    let mut target = RuntimeState::default();

    target.apply_prepared_motion(prepared_motion);

    assert_ne!(source.particles.as_ptr(), target.particles.as_ptr());
    pretty_assertions::assert_eq!(target.particles, expected_particles);
}

#[test]
fn apply_prepared_motion_recycles_previous_particle_capacity_into_preview_scratch() {
    let tracked = location(11, 22, 33, 44);
    let mut source = RuntimeState::default();
    source.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    source.apply_step_output(sample_step_output());

    let mut target = RuntimeState::default();
    target.initialize_cursor(point(13.0, 14.0), default_shape(), 17, &tracked);
    target.apply_step_output(sample_step_output());

    let previous_particles_capacity = target.particles.capacity();
    let previous_particles_ptr = target.particles.as_ptr();

    target.apply_prepared_motion(source.prepared_motion());

    assert_eq!(
        target.preview_particles_scratch_capacity(),
        previous_particles_capacity
    );
    assert_eq!(
        target.preview_particles_scratch_ptr(),
        previous_particles_ptr
    );
}

#[test]
fn runtime_state_clone_resets_retained_preview_particle_scratch() {
    let mut state = RuntimeState::default();
    state.reclaim_preview_particles_scratch(Vec::with_capacity(8));

    let cloned = state.clone();

    assert!(state.preview_particles_scratch_capacity() > 0);
    assert_eq!(cloned.preview_particles_scratch_capacity(), 0);
}

#[test]
fn planning_preview_owns_independent_particle_storage() {
    let tracked = location(11, 22, 33, 44);
    let mut source = RuntimeState::default();
    source.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    source.apply_step_output(sample_step_output());
    let source_particles_ptr = source.particles.as_ptr();
    let expected_particles = source.particles.clone();

    let preview = RuntimePreview::new(&mut source);

    assert_ne!(preview.particles_storage_ptr(), source_particles_ptr);
    pretty_assertions::assert_eq!(preview.particles(), expected_particles.as_slice());
}

#[test]
fn planning_preview_reuses_returned_preview_particle_scratch() {
    let tracked = location(11, 22, 33, 44);
    let mut source = RuntimeState::default();
    source.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    source.apply_step_output(sample_step_output());
    let expected_particles = source.particles.clone();

    let preview = RuntimePreview::new(&mut source);
    let preview_particles_capacity = preview.particles_storage_capacity();
    let preview_particles_ptr = preview.particles_storage_ptr();
    let preview_particles = preview.into_preview_particle_storage();
    source.reclaim_preview_particles_scratch(preview_particles);

    let next_preview = RuntimePreview::new(&mut source);

    assert_eq!(
        next_preview.particles_storage_capacity(),
        preview_particles_capacity
    );
    assert_eq!(next_preview.particles_storage_ptr(), preview_particles_ptr);
    pretty_assertions::assert_eq!(next_preview.particles(), expected_particles.as_slice());
}

#[test]
fn planning_preview_mutation_does_not_write_back_into_source_runtime() {
    let tracked = location(11, 22, 33, 44);
    let mut source = RuntimeState::default();
    source.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    source.set_target(point(8.0, 9.0), default_shape());
    source.start_animation_towards_target();
    source.set_color_at_cursor(Some(0x00AB_CDEF));
    source.record_observed_mode(/*current_is_cmdline*/ true);
    source.apply_step_output(sample_step_output());
    let expected = source.clone();

    let mut preview = RuntimePreview::new(&mut source);
    let preview_runtime = preview.runtime_mut();
    preview_runtime.set_enabled(false);
    preview_runtime.set_target(point(21.0, 34.0), default_shape());
    preview_runtime.set_color_at_cursor(Some(0x0012_3456));
    preview_runtime.record_observed_mode(/*current_is_cmdline*/ false);
    preview_runtime.start_animation_towards_target();

    pretty_assertions::assert_eq!(source, expected);
}

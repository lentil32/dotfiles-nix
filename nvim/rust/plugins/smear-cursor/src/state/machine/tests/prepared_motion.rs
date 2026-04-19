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

    assert!(!target.aggregated_particle_cells_cache_is_dirty());
    assert!(!target.particle_screen_cells_cache_is_dirty());
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
fn planning_preview_borrows_live_particles_until_materialized() {
    let tracked = location(11, 22, 33, 44);
    let mut source = RuntimeState::default();
    source.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    source.apply_step_output(sample_step_output());

    let preview = source.planning_preview();

    assert!(!preview.preview_particles_are_materialized());
    pretty_assertions::assert_eq!(preview.particles(), source.particles());
}

#[test]
fn planning_preview_reuses_returned_preview_particle_scratch() {
    let tracked = location(11, 22, 33, 44);
    let mut source = RuntimeState::default();
    source.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    source.apply_step_output(sample_step_output());

    let preview = source.planning_preview();
    let preview_particles_capacity = preview.particles_storage_capacity();
    let preview_particles_ptr = preview.particles_storage_ptr();
    source.reclaim_preview_particles_scratch(preview.into_preview_particle_storage());

    let next_preview = source.planning_preview();

    assert_eq!(
        next_preview.particles_storage_capacity(),
        preview_particles_capacity
    );
    assert_eq!(next_preview.particles_storage_ptr(), preview_particles_ptr);
    assert!(!next_preview.preview_particles_are_materialized());
    pretty_assertions::assert_eq!(next_preview.particles(), source.particles());
}

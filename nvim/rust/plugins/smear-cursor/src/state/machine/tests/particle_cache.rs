use super::*;
use pretty_assertions::assert_eq;

#[test]
fn aggregated_particle_cache_reuses_cached_cells_until_particles_change() {
    let mut state = RuntimeState::default();
    let tracked = location(10, 20, 1, 1);
    state.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    state.apply_step_output(sample_step_output());

    assert!(!state.has_cached_aggregated_particle_cells());

    let initial_aggregates = state.shared_aggregated_particle_cells();
    assert!(state.has_cached_aggregated_particle_cells());
    assert!(state.particle_aggregation_scratch_index_capacity() > 0);
    assert!(state.particle_aggregation_scratch_cells_capacity() > 0);

    let repeated_aggregates = state.shared_aggregated_particle_cells();
    assert!(std::sync::Arc::ptr_eq(
        &initial_aggregates,
        &repeated_aggregates
    ));

    state.apply_scroll_shift(1.0, -2.0, 0.0, 40.0);
    assert!(!state.has_cached_aggregated_particle_cells());

    let shifted_aggregates = state.shared_aggregated_particle_cells();
    assert!(state.has_cached_aggregated_particle_cells());
    assert!(!std::sync::Arc::ptr_eq(
        &initial_aggregates,
        &shifted_aggregates
    ));
    assert_eq!(state.particles()[0].position, point(5.0, 9.0));
    assert_eq!(
        shifted_aggregates[0].screen_cell(),
        crate::position::ScreenCell::new(5, 9)
    );
}

#[test]
fn particle_screen_cell_cache_reuses_cached_cells_until_particles_change() {
    let mut state = RuntimeState::default();
    let tracked = location(10, 20, 1, 1);
    state.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    state.apply_step_output(sample_step_output());

    assert!(!state.has_cached_particle_screen_cells());

    let initial_screen_cells = state.shared_particle_screen_cells();
    assert!(state.has_cached_aggregated_particle_cells());
    assert!(state.has_cached_particle_screen_cells());
    assert!(state.particle_aggregation_scratch_index_capacity() > 0);
    assert!(state.particle_aggregation_scratch_cells_capacity() > 0);
    assert!(state.particle_aggregation_scratch_screen_cells_capacity() > 0);

    let repeated_screen_cells = state.shared_particle_screen_cells();
    assert!(std::sync::Arc::ptr_eq(
        &initial_screen_cells,
        &repeated_screen_cells
    ));

    state.apply_scroll_shift(1.0, -2.0, 0.0, 40.0);
    assert!(!state.has_cached_aggregated_particle_cells());
    assert!(!state.has_cached_particle_screen_cells());

    let shifted_screen_cells = state.shared_particle_screen_cells();
    assert!(state.has_cached_aggregated_particle_cells());
    assert!(state.has_cached_particle_screen_cells());
    assert!(!std::sync::Arc::ptr_eq(
        &initial_screen_cells,
        &shifted_screen_cells
    ));
    assert_eq!(
        shifted_screen_cells.as_ref(),
        &[crate::position::ScreenCell::new(5, 9).expect("shifted screen cell")]
    );
}

#[test]
fn purging_particle_cache_rebuilds_same_artifacts_for_same_particles() {
    let mut state = RuntimeState::default();
    state.config.particles_over_text = false;
    let tracked = location(10, 20, 1, 1);
    state.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    state.apply_step_output(sample_step_output());

    let cached_aggregates = state.shared_aggregated_particle_cells();
    let cached_screen_cells = state.shared_particle_screen_cells();

    state.purge_cached_particle_artifacts();

    assert!(!state.has_cached_aggregated_particle_cells());
    assert!(!state.has_cached_particle_screen_cells());
    assert_eq!(state.shared_aggregated_particle_cells(), cached_aggregates);
    assert_eq!(state.shared_particle_screen_cells(), cached_screen_cells);
}

#[test]
fn semantic_view_ignores_purgeable_particle_cache_materialization() {
    let mut cold_state = RuntimeState::default();
    cold_state.config.particles_over_text = false;
    let tracked = location(10, 20, 1, 1);
    cold_state.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    cold_state.apply_step_output(sample_step_output());

    let mut warm_state = cold_state.clone();
    let _ = warm_state.shared_particle_screen_cells();

    assert!(!cold_state.has_cached_aggregated_particle_cells());
    assert!(!cold_state.has_cached_particle_screen_cells());
    assert!(warm_state.has_cached_aggregated_particle_cells());
    assert!(warm_state.has_cached_particle_screen_cells());
    assert_eq!(warm_state.semantic_view(), cold_state.semantic_view());
}

#[test]
fn runtime_state_clone_resets_retained_particle_aggregation_scratch() {
    let mut state = RuntimeState::default();
    state.config.particles_over_text = false;
    let tracked = location(10, 20, 1, 1);
    state.initialize_cursor(point(3.0, 4.0), default_shape(), 7, &tracked);
    state.apply_step_output(sample_step_output());

    let _ = state.shared_particle_screen_cells();
    let cloned = state.clone();

    assert!(state.particle_aggregation_scratch_index_capacity() > 0);
    assert!(state.particle_aggregation_scratch_cells_capacity() > 0);
    assert!(state.particle_aggregation_scratch_screen_cells_capacity() > 0);
    assert_eq!(cloned.particle_aggregation_scratch_index_capacity(), 0);
    assert_eq!(cloned.particle_aggregation_scratch_cells_capacity(), 0);
    assert_eq!(
        cloned.particle_aggregation_scratch_screen_cells_capacity(),
        0
    );
}

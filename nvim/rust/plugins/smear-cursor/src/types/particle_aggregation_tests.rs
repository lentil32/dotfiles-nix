use super::Particle;
use super::ParticleAggregationScratch;
use super::ParticleScreenCellsMode;
use super::aggregate_particle_artifacts_with_scratch;
use crate::position::RenderPoint;
use crate::position::ScreenCell;
use pretty_assertions::assert_eq;

#[test]
fn aggregate_particle_artifacts_with_scratch_reuses_retained_buffers() {
    let particles = vec![
        Particle {
            position: RenderPoint { row: 4.1, col: 2.1 },
            velocity: RenderPoint::ZERO,
            lifetime: 2.0,
        },
        Particle {
            position: RenderPoint { row: 3.2, col: 5.4 },
            velocity: RenderPoint::ZERO,
            lifetime: 3.0,
        },
        Particle {
            position: RenderPoint { row: 4.6, col: 2.6 },
            velocity: RenderPoint::ZERO,
            lifetime: 4.0,
        },
    ];
    let mut scratch = ParticleAggregationScratch::default();

    let initial = aggregate_particle_artifacts_with_scratch(
        &particles,
        ParticleScreenCellsMode::Collect,
        &mut scratch,
    );
    let retained_index_capacity = scratch.cell_index_capacity();
    let retained_cells_capacity = scratch.aggregated_cells_capacity();
    let retained_screen_capacity = scratch.particle_screen_cells_capacity();

    assert!(retained_index_capacity > 0);
    assert!(retained_cells_capacity > 0);
    assert!(retained_screen_capacity > 0);
    assert_eq!(
        initial.particle_screen_cells.as_ref(),
        &[
            ScreenCell::new(3, 5).expect("positive row and column"),
            ScreenCell::new(4, 2).expect("positive row and column"),
        ]
    );

    let repeated = aggregate_particle_artifacts_with_scratch(
        &particles,
        ParticleScreenCellsMode::Collect,
        &mut scratch,
    );

    assert_eq!(scratch.cell_index_capacity(), retained_index_capacity);
    assert_eq!(scratch.aggregated_cells_capacity(), retained_cells_capacity);
    assert_eq!(
        scratch.particle_screen_cells_capacity(),
        retained_screen_capacity
    );
    assert_eq!(
        repeated.aggregated_particle_cells.as_ref(),
        initial.aggregated_particle_cells.as_ref()
    );
    assert_eq!(
        repeated.particle_screen_cells.as_ref(),
        initial.particle_screen_cells.as_ref()
    );
}

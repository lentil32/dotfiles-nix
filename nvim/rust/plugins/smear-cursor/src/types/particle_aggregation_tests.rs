use super::AggregatedParticleCell;
use super::Particle;
use super::ParticleAggregationArtifacts;
use super::ParticleAggregationScratch;
use super::ParticleScreenCellsMode;
use super::aggregate_particle_artifacts_with_scratch;
use crate::position::RenderPoint;
use crate::position::ScreenCell;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;
use std::sync::Arc;

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
        initial
            .aggregated_particle_cells
            .iter()
            .map(|cell| (cell.row(), cell.col()))
            .collect::<Vec<_>>(),
        vec![(3, 5), (4, 2)]
    );
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

#[derive(Default)]
struct OrderedSparseParticleAggregationScratch {
    cell_map: BTreeMap<(i64, i64), AggregatedParticleCell>,
    ordered_cells: Vec<AggregatedParticleCell>,
    particle_screen_cells: Vec<ScreenCell>,
}

fn aggregate_particle_artifacts_btree_with_scratch(
    particles: &[Particle],
    screen_cells_mode: ParticleScreenCellsMode,
    scratch: &mut OrderedSparseParticleAggregationScratch,
) -> ParticleAggregationArtifacts {
    if particles.is_empty() {
        scratch.cell_map.clear();
        scratch.ordered_cells.clear();
        scratch.particle_screen_cells.clear();
        return ParticleAggregationArtifacts {
            aggregated_particle_cells: Arc::default(),
            particle_screen_cells: Arc::default(),
        };
    }

    scratch.cell_map.clear();
    scratch.ordered_cells.clear();
    for particle in particles {
        let row = particle.position.row.floor() as i64;
        let col = particle.position.col.floor() as i64;
        let sub_row =
            super::round_lua(4.0 * super::frac01(particle.position.row) + 0.5).clamp(1, 4);
        let sub_col =
            super::round_lua(2.0 * super::frac01(particle.position.col) + 0.5).clamp(1, 2);

        scratch
            .cell_map
            .entry((row, col))
            .or_insert_with(|| AggregatedParticleCell::new(row, col))
            .add_lifetime(
                (sub_row.saturating_sub(1)) as usize,
                (sub_col.saturating_sub(1)) as usize,
                particle.lifetime,
            );
    }

    scratch
        .ordered_cells
        .extend(scratch.cell_map.values().cloned());

    let particle_screen_cells = match screen_cells_mode {
        ParticleScreenCellsMode::Skip => {
            scratch.particle_screen_cells.clear();
            Arc::default()
        }
        ParticleScreenCellsMode::Collect => {
            scratch.particle_screen_cells.clear();
            scratch.particle_screen_cells.extend(
                scratch
                    .ordered_cells
                    .iter()
                    .filter_map(AggregatedParticleCell::screen_cell),
            );
            if scratch.particle_screen_cells.is_empty() {
                Arc::default()
            } else {
                Arc::from(scratch.particle_screen_cells.as_slice())
            }
        }
    };

    ParticleAggregationArtifacts {
        aggregated_particle_cells: Arc::from(scratch.ordered_cells.as_slice()),
        particle_screen_cells,
    }
}

fn particle_aggregation_benchmark_fixture() -> Vec<Particle> {
    let mut particles = Vec::new();
    for row in 1..=48 {
        for col in 1..=24 {
            for sample in 0..4 {
                particles.push(Particle {
                    position: RenderPoint {
                        row: f64::from(row) + 0.13 * f64::from(sample),
                        col: f64::from(col) + 0.27 * f64::from(sample),
                    },
                    velocity: RenderPoint::ZERO,
                    lifetime: 1.0 + f64::from((row + col + sample) % 5),
                });
            }
        }
    }
    particles
}

fn particle_aggregation_benchmark_checksum(artifacts: &ParticleAggregationArtifacts) -> usize {
    let aggregate_checksum = artifacts
        .aggregated_particle_cells
        .iter()
        .map(|cell| {
            cell.row() as usize
                + cell.col() as usize
                + usize::from(cell.dot_count)
                + (cell.lifetime_sum as usize)
        })
        .sum::<usize>();
    let screen_checksum = artifacts
        .particle_screen_cells
        .iter()
        .map(|cell| (cell.row() as usize) + (cell.col() as usize))
        .sum::<usize>();
    aggregate_checksum.saturating_add(screen_checksum)
}

fn run_particle_aggregation_benchmark_case(
    case_name: &str,
    particles: &[Particle],
    iterations: usize,
) {
    let mut retained_vec_scratch = ParticleAggregationScratch::default();
    let mut ordered_sparse_scratch = OrderedSparseParticleAggregationScratch::default();
    let expected = aggregate_particle_artifacts_btree_with_scratch(
        particles,
        ParticleScreenCellsMode::Collect,
        &mut ordered_sparse_scratch,
    );
    let current = aggregate_particle_artifacts_with_scratch(
        particles,
        ParticleScreenCellsMode::Collect,
        &mut retained_vec_scratch,
    );
    assert_eq!(
        current.aggregated_particle_cells.as_ref(),
        expected.aggregated_particle_cells.as_ref()
    );
    assert_eq!(
        current.particle_screen_cells.as_ref(),
        expected.particle_screen_cells.as_ref()
    );

    for implementation in ["retained_vec", "ordered_sparse_btree"] {
        for _ in 0..32 {
            let artifacts = match implementation {
                "retained_vec" => aggregate_particle_artifacts_with_scratch(
                    std::hint::black_box(particles),
                    std::hint::black_box(ParticleScreenCellsMode::Collect),
                    std::hint::black_box(&mut retained_vec_scratch),
                ),
                "ordered_sparse_btree" => aggregate_particle_artifacts_btree_with_scratch(
                    std::hint::black_box(particles),
                    std::hint::black_box(ParticleScreenCellsMode::Collect),
                    std::hint::black_box(&mut ordered_sparse_scratch),
                ),
                _ => unreachable!("unknown benchmark implementation"),
            };
            std::hint::black_box(particle_aggregation_benchmark_checksum(&artifacts));
        }

        let started_at = std::time::Instant::now();
        let mut checksum = 0_usize;
        for _ in 0..iterations {
            let artifacts = match implementation {
                "retained_vec" => aggregate_particle_artifacts_with_scratch(
                    std::hint::black_box(particles),
                    std::hint::black_box(ParticleScreenCellsMode::Collect),
                    std::hint::black_box(&mut retained_vec_scratch),
                ),
                "ordered_sparse_btree" => aggregate_particle_artifacts_btree_with_scratch(
                    std::hint::black_box(particles),
                    std::hint::black_box(ParticleScreenCellsMode::Collect),
                    std::hint::black_box(&mut ordered_sparse_scratch),
                ),
                _ => unreachable!("unknown benchmark implementation"),
            };
            checksum = checksum.saturating_add(particle_aggregation_benchmark_checksum(&artifacts));
            std::hint::black_box(artifacts);
        }

        let elapsed = started_at.elapsed();
        let nanos_per_iteration = elapsed.as_nanos() / (iterations.max(1) as u128);
        println!(
            "benchmark=particle_aggregation case={case_name} impl={implementation} particles={} iterations={iterations} total_ms={} ns_per_iter={} checksum={checksum}",
            particles.len(),
            elapsed.as_millis(),
            nanos_per_iteration,
        );
    }
}

mod benchmarks {
    use super::*;

    #[test]
    #[ignore = "benchmark: run with cargo test -p nvimrs-smear-cursor benchmark_particle_aggregation --release -- --ignored --nocapture"]
    fn benchmark_particle_aggregation_long_animation_fixture() {
        let particles = particle_aggregation_benchmark_fixture();
        run_particle_aggregation_benchmark_case("long_animation_fixture", &particles, 200);
    }
}

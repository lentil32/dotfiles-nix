use crate::core::types::ProjectionPolicyRevision;
use crate::core::types::StrokeId;
use crate::position::RenderPoint;
use crate::position::ScreenCell;
use nvimrs_nvim_utils::mode::is_cmdline_mode;
use nvimrs_nvim_utils::mode::is_insert_like_mode;
use nvimrs_nvim_utils::mode::is_replace_like_mode;
use nvimrs_nvim_utils::mode::is_terminal_like_mode;
use nvimrs_nvim_utils::mode::is_visual_like_mode;
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

pub(crate) const BASE_TIME_INTERVAL: f64 = 1000.0 / 120.0;
pub(crate) const EPSILON: f64 = 1.0e-9;
pub(crate) const DEFAULT_RNG_STATE: u32 = 0xA341_316C;

// Keep only the mode family in the simulation/render hot path; observation
// state retains the raw editor mode string where exact values still matter.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ModeClass {
    NormalLike,
    InsertLike,
    ReplaceLike,
    Cmdline,
    TerminalLike,
    VisualLike,
    Other,
}

impl ModeClass {
    pub(crate) fn from_mode(mode: &str) -> Self {
        if is_insert_like_mode(mode) {
            Self::InsertLike
        } else if is_replace_like_mode(mode) {
            Self::ReplaceLike
        } else if is_cmdline_mode(mode) {
            Self::Cmdline
        } else if is_terminal_like_mode(mode) {
            Self::TerminalLike
        } else if is_visual_like_mode(mode) {
            Self::VisualLike
        } else if mode.starts_with('n') {
            Self::NormalLike
        } else {
            Self::Other
        }
    }

    pub(crate) const fn is_insert_like(self) -> bool {
        matches!(self, Self::InsertLike)
    }
}

impl From<&str> for ModeClass {
    fn from(mode: &str) -> Self {
        Self::from_mode(mode)
    }
}

pub(crate) fn smoothstep01(value: f64) -> f64 {
    let clamped = value.clamp(0.0, 1.0);
    clamped * clamped * (3.0 - 2.0 * clamped)
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum CursorCellShape {
    Block,
    VerticalBar,
    HorizontalBar,
}

impl CursorCellShape {
    pub(crate) fn from_corners(corners: &[RenderPoint; 4]) -> Self {
        let mut min_row = f64::INFINITY;
        let mut max_row = f64::NEG_INFINITY;
        let mut min_col = f64::INFINITY;
        let mut max_col = f64::NEG_INFINITY;
        for corner in corners {
            min_row = min_row.min(corner.row);
            max_row = max_row.max(corner.row);
            min_col = min_col.min(corner.col);
            max_col = max_col.max(corner.col);
        }

        let height = (max_row - min_row).abs();
        let width = (max_col - min_col).abs();
        if width <= (1.0 / 8.0) + EPSILON && height >= 1.0 - EPSILON {
            Self::VerticalBar
        } else if height <= (1.0 / 8.0) + EPSILON && width >= 1.0 - EPSILON {
            Self::HorizontalBar
        } else {
            Self::Block
        }
    }

    pub(crate) const fn glyph(self) -> &'static str {
        match self {
            Self::Block => "█",
            Self::VerticalBar => "▏",
            Self::HorizontalBar => "▁",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Particle {
    pub(crate) position: RenderPoint,
    pub(crate) velocity: RenderPoint,
    pub(crate) lifetime: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AggregatedParticleCell {
    row: i64,
    col: i64,
    cell: [[f64; 2]; 4],
    dot_count: u8,
    lifetime_sum: f64,
}

pub(crate) type SharedAggregatedParticleCells = Arc<[AggregatedParticleCell]>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ParticleScreenCellsMode {
    Skip,
    Collect,
}

#[derive(Debug, Default)]
pub(crate) struct ParticleAggregationScratch {
    cell_index: HashMap<(i64, i64), usize>,
    aggregated_cells: Vec<AggregatedParticleCell>,
    particle_screen_cells: Vec<ScreenCell>,
}

impl ParticleAggregationScratch {
    #[cfg(test)]
    pub(crate) fn cell_index_capacity(&self) -> usize {
        self.cell_index.capacity()
    }

    #[cfg(test)]
    pub(crate) fn aggregated_cells_capacity(&self) -> usize {
        self.aggregated_cells.capacity()
    }

    #[cfg(test)]
    pub(crate) fn particle_screen_cells_capacity(&self) -> usize {
        self.particle_screen_cells.capacity()
    }
}

#[derive(Debug)]
pub(crate) struct ParticleAggregationArtifacts {
    pub(crate) aggregated_particle_cells: SharedAggregatedParticleCells,
    pub(crate) particle_screen_cells: SharedParticleScreenCells,
}

impl AggregatedParticleCell {
    const fn new(row: i64, col: i64) -> Self {
        Self {
            row,
            col,
            cell: [[0.0; 2]; 4],
            dot_count: 0,
            lifetime_sum: 0.0,
        }
    }

    pub(crate) const fn row(&self) -> i64 {
        self.row
    }

    pub(crate) const fn col(&self) -> i64 {
        self.col
    }

    pub(crate) fn screen_cell(&self) -> Option<ScreenCell> {
        ScreenCell::new(self.row, self.col)
    }

    pub(crate) const fn cell(&self) -> &[[f64; 2]; 4] {
        &self.cell
    }

    pub(crate) fn lifetime_average(&self) -> Option<f64> {
        if self.dot_count == 0 {
            return None;
        }
        Some(self.lifetime_sum / f64::from(self.dot_count))
    }

    fn add_lifetime(&mut self, sub_row: usize, sub_col: usize, lifetime: f64) {
        let previous = self.cell[sub_row][sub_col];
        self.cell[sub_row][sub_col] = previous + lifetime;
        self.lifetime_sum += lifetime;

        let was_visible = previous > 0.0;
        let is_visible = self.cell[sub_row][sub_col] > 0.0;
        if was_visible == is_visible {
            return;
        }

        if is_visible {
            self.dot_count = self.dot_count.saturating_add(1);
        } else {
            self.dot_count = self.dot_count.saturating_sub(1);
        }
    }
}

fn frac01(value: f64) -> f64 {
    value.rem_euclid(1.0)
}

fn round_lua(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

pub(crate) fn aggregate_particle_artifacts(
    particles: &[Particle],
    screen_cells_mode: ParticleScreenCellsMode,
) -> ParticleAggregationArtifacts {
    let mut scratch = ParticleAggregationScratch::default();
    aggregate_particle_artifacts_with_scratch(particles, screen_cells_mode, &mut scratch)
}

pub(crate) fn aggregate_particle_artifacts_with_scratch(
    particles: &[Particle],
    screen_cells_mode: ParticleScreenCellsMode,
    scratch: &mut ParticleAggregationScratch,
) -> ParticleAggregationArtifacts {
    if particles.is_empty() {
        scratch.cell_index.clear();
        scratch.aggregated_cells.clear();
        scratch.particle_screen_cells.clear();
        return ParticleAggregationArtifacts {
            aggregated_particle_cells: Arc::default(),
            particle_screen_cells: Arc::default(),
        };
    }

    scratch.cell_index.clear();
    scratch.aggregated_cells.clear();

    for particle in particles {
        let row = particle.position.row.floor() as i64;
        let col = particle.position.col.floor() as i64;
        let sub_row = round_lua(4.0 * frac01(particle.position.row) + 0.5).clamp(1, 4);
        let sub_col = round_lua(2.0 * frac01(particle.position.col) + 0.5).clamp(1, 2);

        let index = match scratch.cell_index.entry((row, col)) {
            std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let index = scratch.aggregated_cells.len();
                scratch
                    .aggregated_cells
                    .push(AggregatedParticleCell::new(row, col));
                entry.insert(index);
                index
            }
        };

        scratch.aggregated_cells[index].add_lifetime(
            (sub_row.saturating_sub(1)) as usize,
            (sub_col.saturating_sub(1)) as usize,
            particle.lifetime,
        );
    }

    scratch
        .aggregated_cells
        .sort_unstable_by_key(|cell| (cell.row(), cell.col()));

    let particle_screen_cells = match screen_cells_mode {
        ParticleScreenCellsMode::Skip => {
            scratch.particle_screen_cells.clear();
            Arc::default()
        }
        ParticleScreenCellsMode::Collect => {
            scratch.particle_screen_cells.clear();
            scratch.particle_screen_cells.extend(
                scratch
                    .aggregated_cells
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
        aggregated_particle_cells: Arc::from(scratch.aggregated_cells.as_slice()),
        particle_screen_cells,
    }
}

pub(crate) fn aggregate_particle_cells(particles: &[Particle]) -> SharedAggregatedParticleCells {
    aggregate_particle_artifacts(particles, ParticleScreenCellsMode::Skip).aggregated_particle_cells
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderStepSample {
    pub(crate) corners: [RenderPoint; 4],
    pub(crate) dt_ms: f64,
}

impl RenderStepSample {
    pub(crate) fn new(corners: [RenderPoint; 4], dt_ms: f64) -> Self {
        let dt_ms = if dt_ms.is_finite() {
            dt_ms.max(0.0)
        } else {
            0.0
        };
        Self { corners, dt_ms }
    }
}

pub(crate) type SharedRenderStepSamples = Arc<[RenderStepSample]>;
pub(crate) type SharedParticles = Arc<Vec<Particle>>;
pub(crate) type SharedParticleScreenCells = Arc<[ScreenCell]>;

pub(crate) fn aggregate_particle_screen_cells(
    aggregated_particle_cells: &[AggregatedParticleCell],
) -> SharedParticleScreenCells {
    if aggregated_particle_cells.is_empty() {
        return Arc::default();
    }

    Arc::from(
        aggregated_particle_cells
            .iter()
            .filter_map(AggregatedParticleCell::screen_cell)
            .collect::<Vec<_>>(),
    )
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StaticRenderConfig {
    pub(crate) cursor_color: Option<String>,
    pub(crate) cursor_color_insert_mode: Option<String>,
    pub(crate) normal_bg: Option<String>,
    pub(crate) transparent_bg_fallback_color: String,
    pub(crate) cterm_cursor_colors: Option<Vec<u16>>,
    pub(crate) cterm_bg: Option<u16>,
    pub(crate) hide_target_hack: bool,
    pub(crate) max_kept_windows: usize,
    pub(crate) never_draw_over_target: bool,
    pub(crate) particle_max_lifetime: f64,
    pub(crate) particle_switch_octant_braille: f64,
    pub(crate) particles_over_text: bool,
    pub(crate) color_levels: u32,
    pub(crate) gamma: f64,
    pub(crate) block_aspect_ratio: f64,
    pub(crate) tail_duration_ms: f64,
    pub(crate) simulation_hz: f64,
    pub(crate) trail_thickness: f64,
    pub(crate) trail_thickness_x: f64,
    pub(crate) spatial_coherence_weight: f64,
    pub(crate) temporal_stability_weight: f64,
    pub(crate) top_k_per_cell: u8,
    pub(crate) windows_zindex: u32,
}

fn particle_artifacts_for_shared_particles(
    particles: SharedParticles,
    particles_over_text: bool,
) -> (
    usize,
    SharedAggregatedParticleCells,
    SharedParticleScreenCells,
) {
    let particle_count = particles.len();
    let artifacts = aggregate_particle_artifacts(
        particles.as_slice(),
        if particles_over_text {
            ParticleScreenCellsMode::Skip
        } else {
            ParticleScreenCellsMode::Collect
        },
    );
    (
        particle_count,
        artifacts.aggregated_particle_cells,
        artifacts.particle_screen_cells,
    )
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderFrame {
    pub(crate) mode: ModeClass,
    pub(crate) corners: [RenderPoint; 4],
    pub(crate) step_samples: SharedRenderStepSamples,
    pub(crate) planner_idle_steps: u32,
    pub(crate) target: RenderPoint,
    pub(crate) target_corners: [RenderPoint; 4],
    pub(crate) vertical_bar: bool,
    pub(crate) trail_stroke_id: StrokeId,
    pub(crate) retarget_epoch: u64,
    pub(crate) particle_count: usize,
    pub(crate) aggregated_particle_cells: SharedAggregatedParticleCells,
    pub(crate) particle_screen_cells: SharedParticleScreenCells,
    pub(crate) color_at_cursor: Option<u32>,
    pub(crate) projection_policy_revision: ProjectionPolicyRevision,
    pub(crate) static_config: Arc<StaticRenderConfig>,
}

impl Deref for RenderFrame {
    type Target = StaticRenderConfig;

    fn deref(&self) -> &Self::Target {
        &self.static_config
    }
}

impl RenderFrame {
    pub(crate) const fn has_particles(&self) -> bool {
        self.particle_count > 0
    }

    pub(crate) fn aggregated_particle_cells(&self) -> &[AggregatedParticleCell] {
        &self.aggregated_particle_cells
    }

    pub(crate) fn set_particles(&mut self, particles: SharedParticles) {
        let (particle_count, aggregated_particle_cells, particle_screen_cells) =
            particle_artifacts_for_shared_particles(particles, self.particles_over_text);
        self.particle_count = particle_count;
        self.aggregated_particle_cells = aggregated_particle_cells;
        self.particle_screen_cells = particle_screen_cells;
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StepInput {
    pub(crate) mode: ModeClass,
    pub(crate) time_interval: f64,
    pub(crate) config_time_interval: f64,
    pub(crate) head_response_ms: f64,
    pub(crate) damping_ratio: f64,
    pub(crate) current_corners: [RenderPoint; 4],
    pub(crate) trail_origin_corners: [RenderPoint; 4],
    pub(crate) target_corners: [RenderPoint; 4],
    pub(crate) spring_velocity_corners: [RenderPoint; 4],
    pub(crate) trail_elapsed_ms: [f64; 4],
    pub(crate) max_length: f64,
    pub(crate) max_length_insert_mode: f64,
    pub(crate) trail_duration_ms: f64,
    pub(crate) trail_min_distance: f64,
    pub(crate) trail_thickness: f64,
    pub(crate) trail_thickness_x: f64,
    pub(crate) particles: Vec<Particle>,
    pub(crate) previous_center: RenderPoint,
    pub(crate) particle_damping: f64,
    pub(crate) particles_enabled: bool,
    pub(crate) particle_gravity: f64,
    pub(crate) particle_random_velocity: f64,
    pub(crate) particle_max_num: usize,
    pub(crate) particle_spread: f64,
    pub(crate) particles_per_second: f64,
    pub(crate) particles_per_length: f64,
    pub(crate) particle_max_initial_velocity: f64,
    pub(crate) particle_velocity_from_cursor: f64,
    pub(crate) particle_max_lifetime: f64,
    pub(crate) particle_lifetime_distribution_exponent: f64,
    pub(crate) min_distance_emit_particles: f64,
    pub(crate) vertical_bar: bool,
    pub(crate) horizontal_bar: bool,
    pub(crate) block_aspect_ratio: f64,
    pub(crate) rng_state: u32,
}

#[derive(Debug)]
pub(crate) struct StepOutput {
    pub(crate) current_corners: [RenderPoint; 4],
    pub(crate) velocity_corners: [RenderPoint; 4],
    pub(crate) spring_velocity_corners: [RenderPoint; 4],
    pub(crate) trail_elapsed_ms: [f64; 4],
    pub(crate) particles: Vec<Particle>,
    pub(crate) previous_center: RenderPoint,
    pub(crate) index_head: usize,
    pub(crate) index_tail: usize,
    pub(crate) rng_state: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Rng32 {
    state: u32,
}

impl Rng32 {
    pub(crate) fn from_seed(seed: u32) -> Self {
        let normalized = if seed == 0 { DEFAULT_RNG_STATE } else { seed };
        Self { state: normalized }
    }

    pub(crate) fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        if x == 0 {
            x = DEFAULT_RNG_STATE;
        }
        self.state = x;
        x
    }

    pub(crate) fn next_unit(&mut self) -> f64 {
        f64::from(self.next_u32()) / f64::from(u32::MAX)
    }

    pub(crate) fn state(self) -> u32 {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::ModeClass;
    use super::Particle;
    use super::aggregate_particle_cells;
    use crate::position::RenderPoint;
    use pretty_assertions::assert_eq;

    #[test]
    fn aggregate_particle_cells_orders_and_coalesces_particle_overlays() {
        let aggregated = aggregate_particle_cells(&[
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
        ]);

        assert_eq!(
            aggregated
                .iter()
                .map(|cell| (cell.row(), cell.col()))
                .collect::<Vec<_>>(),
            vec![(3, 5), (4, 2)]
        );
        assert_eq!(aggregated[0].lifetime_average(), Some(3.0));
        assert_eq!(aggregated[1].lifetime_average(), Some(3.0));
    }

    #[test]
    fn mode_class_groups_relevant_editor_mode_families() {
        assert_eq!(ModeClass::from_mode("n"), ModeClass::NormalLike);
        assert_eq!(ModeClass::from_mode("niI"), ModeClass::NormalLike);
        assert_eq!(ModeClass::from_mode("i"), ModeClass::InsertLike);
        assert_eq!(ModeClass::from_mode("ix"), ModeClass::InsertLike);
        assert_eq!(ModeClass::from_mode("R"), ModeClass::ReplaceLike);
        assert_eq!(ModeClass::from_mode("Rc"), ModeClass::ReplaceLike);
        assert_eq!(ModeClass::from_mode("c"), ModeClass::Cmdline);
        assert_eq!(ModeClass::from_mode("cv"), ModeClass::Cmdline);
        assert_eq!(ModeClass::from_mode("nt"), ModeClass::TerminalLike);
        assert_eq!(ModeClass::from_mode("t"), ModeClass::TerminalLike);
        assert_eq!(ModeClass::from_mode("v"), ModeClass::VisualLike);
        assert_eq!(ModeClass::from_mode("\u{16}"), ModeClass::VisualLike);
        assert_eq!(ModeClass::from_mode("s"), ModeClass::Other);
    }
}

#[cfg(test)]
mod particle_aggregation_tests;

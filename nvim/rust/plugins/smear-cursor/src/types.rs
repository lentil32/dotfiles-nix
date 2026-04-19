use crate::core::types::StrokeId;
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

pub(crate) fn display_metric_row_scale(block_aspect_ratio: f64) -> f64 {
    if block_aspect_ratio.is_finite() {
        block_aspect_ratio.abs().max(EPSILON)
    } else {
        1.0
    }
}

pub(crate) fn smoothstep01(value: f64) -> f64 {
    let clamped = value.clamp(0.0, 1.0);
    clamped * clamped * (3.0 - 2.0 * clamped)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Point {
    pub(crate) row: f64,
    pub(crate) col: f64,
}

impl Point {
    pub(crate) const ZERO: Self = Self { row: 0.0, col: 0.0 };

    pub(crate) fn distance_squared(self, other: Self) -> f64 {
        let dy = self.row - other.row;
        let dx = self.col - other.col;
        dy * dy + dx * dx
    }

    pub(crate) fn display_distance_squared(self, other: Self, block_aspect_ratio: f64) -> f64 {
        let dy = (self.row - other.row) * display_metric_row_scale(block_aspect_ratio);
        let dx = self.col - other.col;
        dy * dy + dx * dx
    }

    pub(crate) fn display_distance(self, other: Self, block_aspect_ratio: f64) -> f64 {
        self.display_distance_squared(other, block_aspect_ratio)
            .sqrt()
    }
}

pub(crate) fn corners_center(corners: &[Point; 4]) -> Point {
    let mut row = 0.0_f64;
    let mut col = 0.0_f64;
    for point in corners {
        row += point.row;
        col += point.col;
    }
    Point {
        row: row / 4.0,
        col: col / 4.0,
    }
}

pub(crate) fn current_visual_cursor_anchor(
    current_corners: &[Point; 4],
    target_corners: &[Point; 4],
    target_position: Point,
) -> Point {
    let current_center = corners_center(current_corners);
    let target_center = corners_center(target_corners);
    Point {
        row: target_position.row + (current_center.row - target_center.row),
        col: target_position.col + (current_center.col - target_center.col),
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ScreenCell {
    row: i64,
    col: i64,
}

impl ScreenCell {
    pub(crate) fn new(row: i64, col: i64) -> Option<Self> {
        if row < 1 || col < 1 {
            return None;
        }
        Some(Self { row, col })
    }

    pub(crate) fn from_rounded_point(point: Point) -> Option<Self> {
        if !point.row.is_finite() || !point.col.is_finite() {
            return None;
        }

        let rounded_row = point.row.round();
        let rounded_col = point.col.round();
        if rounded_row < 1.0
            || rounded_col < 1.0
            || rounded_row > i64::MAX as f64
            || rounded_col > i64::MAX as f64
        {
            return None;
        }

        Self::new(rounded_row as i64, rounded_col as i64)
    }

    pub(crate) fn from_visual_cursor_anchor(
        current_corners: &[Point; 4],
        target_corners: &[Point; 4],
        target_position: Point,
    ) -> Option<Self> {
        Self::from_rounded_point(current_visual_cursor_anchor(
            current_corners,
            target_corners,
            target_position,
        ))
    }

    pub(crate) const fn row(self) -> i64 {
        self.row
    }

    pub(crate) const fn col(self) -> i64 {
        self.col
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum CursorCellShape {
    Block,
    VerticalBar,
    HorizontalBar,
}

impl CursorCellShape {
    pub(crate) fn from_corners(corners: &[Point; 4]) -> Self {
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
    pub(crate) position: Point,
    pub(crate) velocity: Point,
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
    pub(crate) corners: [Point; 4],
    pub(crate) dt_ms: f64,
}

impl RenderStepSample {
    pub(crate) fn new(corners: [Point; 4], dt_ms: f64) -> Self {
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
pub(crate) struct PlannerRenderConfig {
    pub(crate) hide_target_hack: bool,
    pub(crate) max_kept_windows: usize,
    pub(crate) never_draw_over_target: bool,
    pub(crate) particle_max_lifetime: f64,
    pub(crate) particle_switch_octant_braille: f64,
    pub(crate) particles_over_text: bool,
    pub(crate) color_levels: u32,
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

impl From<&StaticRenderConfig> for PlannerRenderConfig {
    fn from(config: &StaticRenderConfig) -> Self {
        Self {
            hide_target_hack: config.hide_target_hack,
            max_kept_windows: config.max_kept_windows,
            never_draw_over_target: config.never_draw_over_target,
            particle_max_lifetime: config.particle_max_lifetime,
            particle_switch_octant_braille: config.particle_switch_octant_braille,
            particles_over_text: config.particles_over_text,
            color_levels: config.color_levels,
            block_aspect_ratio: config.block_aspect_ratio,
            tail_duration_ms: config.tail_duration_ms,
            simulation_hz: config.simulation_hz,
            trail_thickness: config.trail_thickness,
            trail_thickness_x: config.trail_thickness_x,
            spatial_coherence_weight: config.spatial_coherence_weight,
            temporal_stability_weight: config.temporal_stability_weight,
            top_k_per_cell: config.top_k_per_cell,
            windows_zindex: config.windows_zindex,
        }
    }
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

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlannerFrame {
    pub(crate) mode: ModeClass,
    pub(crate) corners: [Point; 4],
    pub(crate) step_samples: SharedRenderStepSamples,
    pub(crate) planner_idle_steps: u32,
    pub(crate) target: Point,
    pub(crate) target_corners: [Point; 4],
    pub(crate) vertical_bar: bool,
    pub(crate) trail_stroke_id: StrokeId,
    pub(crate) retarget_epoch: u64,
    pub(crate) particle_count: usize,
    pub(crate) aggregated_particle_cells: SharedAggregatedParticleCells,
    pub(crate) planner_config: PlannerRenderConfig,
}

impl Deref for PlannerFrame {
    type Target = PlannerRenderConfig;

    fn deref(&self) -> &Self::Target {
        &self.planner_config
    }
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

impl PlannerFrame {
    pub(crate) fn from_render_frame(frame: &RenderFrame) -> Self {
        Self {
            mode: frame.mode,
            corners: frame.corners,
            step_samples: frame.step_samples.clone(),
            planner_idle_steps: frame.planner_idle_steps,
            target: frame.target,
            target_corners: frame.target_corners,
            vertical_bar: frame.vertical_bar,
            trail_stroke_id: frame.trail_stroke_id,
            retarget_epoch: frame.retarget_epoch,
            particle_count: frame.particle_count,
            aggregated_particle_cells: frame.aggregated_particle_cells.clone(),
            planner_config: PlannerRenderConfig::from(frame.static_config.as_ref()),
        }
    }

    pub(crate) const fn has_particles(&self) -> bool {
        self.particle_count > 0
    }

    pub(crate) fn aggregated_particle_cells(&self) -> &[AggregatedParticleCell] {
        &self.aggregated_particle_cells
    }

    #[cfg(test)]
    pub(crate) fn set_particles(&mut self, particles: SharedParticles) {
        let (particle_count, aggregated_particle_cells, _) =
            particle_artifacts_for_shared_particles(particles, self.particles_over_text);
        self.particle_count = particle_count;
        self.aggregated_particle_cells = aggregated_particle_cells;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderFrame {
    pub(crate) mode: ModeClass,
    pub(crate) corners: [Point; 4],
    pub(crate) step_samples: SharedRenderStepSamples,
    pub(crate) planner_idle_steps: u32,
    pub(crate) target: Point,
    pub(crate) target_corners: [Point; 4],
    pub(crate) vertical_bar: bool,
    pub(crate) trail_stroke_id: StrokeId,
    pub(crate) retarget_epoch: u64,
    pub(crate) particle_count: usize,
    pub(crate) aggregated_particle_cells: SharedAggregatedParticleCells,
    pub(crate) particle_screen_cells: SharedParticleScreenCells,
    pub(crate) color_at_cursor: Option<u32>,
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
    pub(crate) current_corners: [Point; 4],
    pub(crate) trail_origin_corners: [Point; 4],
    pub(crate) target_corners: [Point; 4],
    pub(crate) spring_velocity_corners: [Point; 4],
    pub(crate) trail_elapsed_ms: [f64; 4],
    pub(crate) max_length: f64,
    pub(crate) max_length_insert_mode: f64,
    pub(crate) trail_duration_ms: f64,
    pub(crate) trail_min_distance: f64,
    pub(crate) trail_thickness: f64,
    pub(crate) trail_thickness_x: f64,
    pub(crate) particles: Vec<Particle>,
    pub(crate) previous_center: Point,
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
    pub(crate) current_corners: [Point; 4],
    pub(crate) velocity_corners: [Point; 4],
    pub(crate) spring_velocity_corners: [Point; 4],
    pub(crate) trail_elapsed_ms: [f64; 4],
    pub(crate) particles: Vec<Particle>,
    pub(crate) previous_center: Point,
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
    use super::Point;
    use super::ScreenCell;
    use super::aggregate_particle_cells;
    use super::current_visual_cursor_anchor;
    use crate::animation::scaled_corners_for_trail;
    use crate::test_support::proptest::DEFAULT_FLOAT_EPSILON;
    use crate::test_support::proptest::approx_eq_point;
    use crate::test_support::proptest::cursor_rectangle;
    use crate::test_support::proptest::positive_scale;
    use crate::test_support::proptest::pure_config;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn expected_screen_cell_from_point(point: Point) -> Option<ScreenCell> {
        if !point.row.is_finite() || !point.col.is_finite() {
            return None;
        }

        let rounded_row = point.row.round();
        let rounded_col = point.col.round();
        if rounded_row < 1.0
            || rounded_col < 1.0
            || rounded_row > i64::MAX as f64
            || rounded_col > i64::MAX as f64
        {
            return None;
        }

        Some(ScreenCell {
            row: rounded_row as i64,
            col: rounded_col as i64,
        })
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_screen_cell_validates_one_indexed_cells(
            row in -8_i64..8_i64,
            col in -8_i64..8_i64,
        ) {
            let expected = if row >= 1 && col >= 1 {
                Some(ScreenCell { row, col })
            } else {
                None
            };

            prop_assert_eq!(ScreenCell::new(row, col), expected);
        }

        #[test]
        fn prop_screen_cell_from_rounded_point_matches_rounding_and_validity_rules(
            row in prop_oneof![
                -4096.0_f64..4096.0_f64,
                Just(f64::NAN),
                Just(f64::INFINITY),
                Just(f64::NEG_INFINITY),
            ],
            col in prop_oneof![
                -4096.0_f64..4096.0_f64,
                Just(f64::NAN),
                Just(f64::INFINITY),
                Just(f64::NEG_INFINITY),
            ],
        ) {
            let point = Point { row, col };
            prop_assert_eq!(
                ScreenCell::from_rounded_point(point),
                expected_screen_cell_from_point(point)
            );
        }

        #[test]
        fn prop_visual_cursor_anchor_stays_on_target_under_scaled_head_geometry(
            fixture in cursor_rectangle(),
            row_scale in positive_scale(),
            col_scale in positive_scale(),
        ) {
            let scaled_corners =
                scaled_corners_for_trail(&fixture.corners, row_scale, col_scale);
            let anchor = current_visual_cursor_anchor(
                &scaled_corners,
                &fixture.corners,
                fixture.position,
            );

            prop_assert!(
                approx_eq_point(anchor, fixture.position, DEFAULT_FLOAT_EPSILON),
                "anchor={anchor:?} target={:?} row_scale={row_scale} col_scale={col_scale}",
                fixture.position
            );
            prop_assert_eq!(
                ScreenCell::from_visual_cursor_anchor(
                    &scaled_corners,
                    &fixture.corners,
                    fixture.position,
                ),
                ScreenCell::new(
                    fixture.position.row.round() as i64,
                    fixture.position.col.round() as i64,
                )
            );
        }
    }

    #[test]
    fn aggregate_particle_cells_orders_and_coalesces_particle_overlays() {
        let aggregated = aggregate_particle_cells(&[
            Particle {
                position: Point { row: 4.1, col: 2.1 },
                velocity: Point::ZERO,
                lifetime: 2.0,
            },
            Particle {
                position: Point { row: 3.2, col: 5.4 },
                velocity: Point::ZERO,
                lifetime: 3.0,
            },
            Particle {
                position: Point { row: 4.6, col: 2.6 },
                velocity: Point::ZERO,
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

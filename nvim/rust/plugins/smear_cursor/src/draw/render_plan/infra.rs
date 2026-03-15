use super::PARTICLE_ZINDEX_OFFSET;
use crate::core::types::{ArcLenQ16, StepIndex, StrokeId};
use crate::draw::BRAILLE_CODE_MIN;
use crate::octant_chars::OCTANT_CHARACTERS;
use crate::types::{Point, RenderFrame, RenderStepSample};
use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::num::NonZeroU32;
use std::sync::{Arc, LazyLock};

#[path = "../render/cell_draw.rs"]
mod cell_draw;
#[path = "../render/geometry.rs"]
mod geometry;
#[path = "../render/latent_field.rs"]
mod latent_field;
#[path = "../render/particles.rs"]
mod particles;

use latent_field::{
    AgeMoment, CompiledCell, DepositedSlice, LatentFieldCache, MICRO_H, MICRO_TILE_SAMPLES,
    MICRO_W, MicroTile, TailBand,
};
use particles::draw_particles;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct HighlightLevel(NonZeroU32);

impl HighlightLevel {
    pub(crate) fn try_new(value: u32) -> Option<Self> {
        NonZeroU32::new(value).map(Self)
    }

    pub(crate) fn from_raw_clamped(value: u32) -> Self {
        let clamped = value.max(1);
        match NonZeroU32::new(clamped) {
            Some(value) => Self(value),
            None => Self(NonZeroU32::MIN),
        }
    }

    pub(crate) fn value(self) -> u32 {
        self.0.get()
    }

    pub(crate) fn abs_diff(self, other: Self) -> u32 {
        self.value().abs_diff(other.value())
    }

    pub(crate) fn index_for_len(self, len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        let raw_index = match usize::try_from(self.value()) {
            Ok(index) => index,
            Err(_) => usize::MAX,
        };
        raw_index.min(len.saturating_sub(1))
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum HighlightRef {
    Normal(HighlightLevel),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Glyph {
    Static(&'static str),
    Braille(u8),
}

impl Glyph {
    pub(crate) const BLOCK: Self = Self::Static("█");

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Static(value) => value,
            Self::Braille(index) => match BRAILLE_GLYPHS.get(index.saturating_sub(1) as usize) {
                Some(value) => value.as_ref(),
                None => "",
            },
        }
    }
}

// Comment: cache boxed braille strings once so `Glyph::Braille` can hand extmark writes a
// stable `&str` without turning the lookup table into a process-lifetime leak.
static BRAILLE_GLYPHS: LazyLock<Box<[Box<str>]>> = LazyLock::new(|| {
    (1_u32..=255_u32)
        .filter_map(|index| {
            char::from_u32((BRAILLE_CODE_MIN as u32).saturating_add(index))
                .map(|character| character.to_string().into_boxed_str())
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
});

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CellOp {
    pub(crate) row: i64,
    pub(crate) col: i64,
    pub(crate) zindex: u32,
    pub(crate) glyph: Glyph,
    pub(crate) highlight: HighlightRef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ParticleOp {
    pub(crate) cell: CellOp,
    pub(crate) requires_background_probe: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TargetCellOverlay {
    pub(crate) row: i64,
    pub(crate) col: i64,
    pub(crate) zindex: u32,
    pub(crate) level: HighlightLevel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ClearOp {
    pub(crate) max_kept_windows: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RenderPlan {
    pub(crate) clear: Option<ClearOp>,
    pub(crate) cell_ops: Vec<CellOp>,
    pub(crate) particle_ops: Vec<ParticleOp>,
    pub(crate) target_cell_overlay: Option<TargetCellOverlay>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DecodePathTrace {
    Baseline,
    PairwiseFallbackDisconnected,
    PairwiseFallbackOversized,
    RibbonDp,
    RibbonDpSolveFailed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct DecodePathCounters {
    pub(crate) baseline: u64,
    pub(crate) pairwise_fallback_disconnected: u64,
    pub(crate) pairwise_fallback_oversized: u64,
    pub(crate) ribbon_dp: u64,
    pub(crate) ribbon_dp_solve_failed: u64,
}

impl DecodePathCounters {
    pub(crate) const fn count_for(self, path: DecodePathTrace) -> u64 {
        match path {
            DecodePathTrace::Baseline => self.baseline,
            DecodePathTrace::PairwiseFallbackDisconnected => self.pairwise_fallback_disconnected,
            DecodePathTrace::PairwiseFallbackOversized => self.pairwise_fallback_oversized,
            DecodePathTrace::RibbonDp => self.ribbon_dp,
            DecodePathTrace::RibbonDpSolveFailed => self.ribbon_dp_solve_failed,
        }
    }

    pub(crate) const fn total(self) -> u64 {
        self.baseline
            .saturating_add(self.pairwise_fallback_disconnected)
            .saturating_add(self.pairwise_fallback_oversized)
            .saturating_add(self.ribbon_dp)
            .saturating_add(self.ribbon_dp_solve_failed)
    }

    fn record(&mut self, path: DecodePathTrace) {
        match path {
            DecodePathTrace::Baseline => {
                self.baseline = self.baseline.saturating_add(1);
            }
            DecodePathTrace::PairwiseFallbackDisconnected => {
                self.pairwise_fallback_disconnected =
                    self.pairwise_fallback_disconnected.saturating_add(1);
            }
            DecodePathTrace::PairwiseFallbackOversized => {
                self.pairwise_fallback_oversized =
                    self.pairwise_fallback_oversized.saturating_add(1);
            }
            DecodePathTrace::RibbonDp => {
                self.ribbon_dp = self.ribbon_dp.saturating_add(1);
            }
            DecodePathTrace::RibbonDpSolveFailed => {
                self.ribbon_dp_solve_failed = self.ribbon_dp_solve_failed.saturating_add(1);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct DecodeDiagnostics {
    pub(crate) last_path: Option<DecodePathTrace>,
    pub(crate) path_counters: DecodePathCounters,
}

impl DecodeDiagnostics {
    fn record_path(&mut self, path: DecodePathTrace) {
        self.last_path = Some(path);
        self.path_counters.record(path);
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DecodedCellState {
    glyph: DecodedGlyph,
    level: HighlightLevel,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum DecodedGlyph {
    Block,
    Matrix(u8),
    Octant(u8),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PlannerState {
    step_index: StepIndex,
    arc_len_q16: ArcLenQ16,
    last_trail_stroke_id: Option<StrokeId>,
    last_pose: Option<latent_field::Pose>,
    history_revision: u64,
    history: VecDeque<DepositedSlice>,
    latent_cache: LatentFieldCache,
    center_history: VecDeque<CenterPathSample>,
    previous_cells: BTreeMap<(i64, i64), DecodedCellState>,
    decode_diagnostics: DecodeDiagnostics,
    compiled_cache: CompiledFieldCache,
}

impl PlannerState {
    pub(crate) const fn step_index(&self) -> StepIndex {
        self.step_index
    }

    pub(crate) const fn history_revision(&self) -> u64 {
        self.history_revision
    }

    pub(crate) const fn decode_diagnostics(&self) -> DecodeDiagnostics {
        self.decode_diagnostics
    }

    fn record_decode_path(&mut self, path: DecodePathTrace) {
        self.decode_diagnostics.record_path(path);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Viewport {
    pub(crate) max_row: i64,
    pub(crate) max_col: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct PlannerOutput {
    pub(crate) plan: RenderPlan,
    pub(crate) next_state: PlannerState,
    pub(crate) signature: Option<u64>,
    pub(crate) decode_path: DecodePathTrace,
}

#[derive(Clone, Debug)]
pub(crate) struct CompiledPlannerFrame {
    next_state: PlannerState,
    compiled: Arc<BTreeMap<(i64, i64), CompiledCell>>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct CompiledFieldCache {
    latest_step: Option<StepIndex>,
    history_revision: u64,
    field: Arc<BTreeMap<(i64, i64), CompiledCell>>,
}

pub(crate) struct PlanBuilder {
    viewport: Viewport,
    cell_ops: Vec<CellOp>,
    particle_ops: Vec<ParticleOp>,
    punch_through_cell: Option<(i64, i64)>,
}

impl PlanBuilder {
    fn with_capacity(
        viewport: Viewport,
        estimated_cells: usize,
        estimated_particles: usize,
    ) -> Self {
        Self {
            viewport,
            cell_ops: Vec::with_capacity(estimated_cells),
            particle_ops: Vec::with_capacity(estimated_particles),
            punch_through_cell: None,
        }
    }

    fn in_bounds(&self, row: i64, col: i64) -> bool {
        row >= 1 && row <= self.viewport.max_row && col >= 1 && col <= self.viewport.max_col
    }

    fn set_punch_through_cell(&mut self, row: i64, col: i64) {
        self.punch_through_cell = Some((row, col));
    }

    fn is_punch_through_cell(&self, row: i64, col: i64) -> bool {
        self.punch_through_cell
            .is_some_and(|(target_row, target_col)| row == target_row && col == target_col)
    }

    pub(super) fn push_cell(
        &mut self,
        row: i64,
        col: i64,
        zindex: u32,
        glyph: Glyph,
        highlight: HighlightRef,
    ) -> bool {
        if !self.in_bounds(row, col) {
            return false;
        }
        if self.is_punch_through_cell(row, col) {
            return false;
        }
        self.cell_ops.push(CellOp {
            row,
            col,
            zindex,
            glyph,
            highlight,
        });
        true
    }

    pub(super) fn push_particle(
        &mut self,
        row: i64,
        col: i64,
        zindex: u32,
        glyph: Glyph,
        highlight: HighlightRef,
        requires_background_probe: bool,
    ) -> bool {
        if !self.in_bounds(row, col) {
            return false;
        }
        if self.is_punch_through_cell(row, col) {
            return false;
        }
        self.particle_ops.push(ParticleOp {
            cell: CellOp {
                row,
                col,
                zindex,
                glyph,
                highlight,
            },
            requires_background_probe,
        });
        true
    }

    fn finish(
        self,
        clear: Option<ClearOp>,
        target_cell_overlay: Option<TargetCellOverlay>,
    ) -> RenderPlan {
        RenderPlan {
            clear,
            cell_ops: self.cell_ops,
            particle_ops: self.particle_ops,
            target_cell_overlay,
        }
    }
}

pub(super) struct PlanResources<'a> {
    pub(super) builder: &'a mut PlanBuilder,
    pub(super) windows_zindex: u32,
    pub(super) particle_zindex: u32,
}

#[derive(Clone, Copy, Debug)]
struct GlyphProfile {
    glyph: DecodedGlyph,
    occupancy: [u8; MICRO_TILE_SAMPLES],
    sample_count: u16,
    complexity: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ShadeProfile {
    level: HighlightLevel,
    sample_q12: u16,
}

#[derive(Clone, Copy, Debug)]
struct LocalCellProfile {
    state: DecodedCellState,
    glyph: GlyphProfile,
    shade: ShadeProfile,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CellCandidate {
    state: Option<DecodedCellState>,
    unary_cost: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CenterPathSample {
    step_index: StepIndex,
    pos: Point,
}

#[derive(Clone, Copy, Debug)]
struct CenterSample {
    pos: Point,
    tangent_row: f64,
    tangent_col: f64,
}

#[derive(Clone, Debug)]
struct SliceCell {
    coord: (i64, i64),
    normal_center_q16: i32,
    normal_span_q16: ProjectedSpanQ16,
    empty_cost: u64,
    non_empty_candidates: Vec<CellCandidate>,
}

impl SliceCell {
    fn new(
        coord: (i64, i64),
        normal_center_q16: i32,
        normal_half_span_q16: i32,
        empty_cost: u64,
        non_empty_candidates: Vec<CellCandidate>,
    ) -> Self {
        Self {
            coord,
            normal_center_q16,
            normal_span_q16: ProjectedSpanQ16::from_center_and_half_span(
                normal_center_q16,
                normal_half_span_q16,
            ),
            empty_cost,
            non_empty_candidates,
        }
    }
}

#[derive(Clone, Debug)]
struct RibbonSlice {
    cells: Vec<SliceCell>,
    tail_u: f64,
    target_width_cells: f64,
    tip_width_cap_cells: f64,
    transverse_width_penalty: f64,
}

impl RibbonSlice {
    fn run_projected_span_q16(&self, state: SliceState) -> Option<ProjectedSpanQ16> {
        let run = state.run?;
        Some(
            self.cells[run.start]
                .normal_span_q16
                .cover(self.cells[run.end].normal_span_q16),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SliceState {
    // NOTE: Keep run bounds as one optional value so partial ranges are unrepresentable.
    run: Option<RunSpan>,
    candidate_offsets: [u8; RIBBON_MAX_RUN_LENGTH],
    local_cost: u64,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RunSpan {
    start: usize,
    end: usize,
}

impl RunSpan {
    fn try_new(start: usize, end: usize) -> Option<Self> {
        (start <= end).then_some(Self { start, end })
    }

    fn contains(self, cell_index: usize) -> bool {
        cell_index >= self.start && cell_index <= self.end
    }
}

impl SliceState {
    fn empty(local_cost: u64) -> Self {
        Self {
            run: None,
            candidate_offsets: [0; RIBBON_MAX_RUN_LENGTH],
            local_cost,
        }
    }

    fn with_run(
        run: RunSpan,
        candidate_offsets: [u8; RIBBON_MAX_RUN_LENGTH],
        local_cost: u64,
    ) -> Self {
        Self {
            run: Some(run),
            candidate_offsets,
            local_cost,
        }
    }

    fn run_start_key(self) -> Option<usize> {
        self.run.map(|run| run.start)
    }

    fn candidate_offset_for(self, cell_index: usize) -> Option<usize> {
        let run = self.run?;
        if !run.contains(cell_index) {
            return None;
        }
        Some(usize::from(self.candidate_offsets[cell_index - run.start]))
    }

    fn tie_break_key(self) -> (Option<RunSpan>, [u8; RIBBON_MAX_RUN_LENGTH]) {
        (self.run, self.candidate_offsets)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProjectedSpanQ16 {
    min_q16: i32,
    max_q16: i32,
}

impl ProjectedSpanQ16 {
    fn try_new(min_q16: i32, max_q16: i32) -> Option<Self> {
        (min_q16 <= max_q16).then_some(Self { min_q16, max_q16 })
    }

    fn from_center_and_half_span(center_q16: i32, half_span_q16: i32) -> Self {
        let safe_half_span_q16 = half_span_q16.max(0);
        let min_q16 = saturating_q16_offset(center_q16, -safe_half_span_q16);
        let max_q16 = saturating_q16_offset(center_q16, safe_half_span_q16);
        match Self::try_new(min_q16, max_q16) {
            Some(span) => span,
            None => Self {
                min_q16: max_q16,
                max_q16: min_q16,
            },
        }
    }

    fn width_cells(self) -> f64 {
        f64::from(self.max_q16.abs_diff(self.min_q16)) / Q16_SCALE as f64
    }

    fn center_q16(self) -> i32 {
        ((i64::from(self.min_q16) + i64::from(self.max_q16)) / 2) as i32
    }

    fn cover(self, other: Self) -> Self {
        Self {
            min_q16: self.min_q16.min(other.min_q16),
            max_q16: self.max_q16.max(other.max_q16),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct VoteStats {
    count: u32,
    total_cost: u64,
}

const MATRIX_CHARACTERS: [&str; 16] = [
    "", "▘", "▝", "▀", "▖", "▌", "▞", "▛", "▗", "▚", "▐", "▜", "▄", "▙", "▟", "█",
];
const MIN_VISIBLE_SAMPLE_Q12: u16 = 6;
const SHADE_PROFILE_NEIGHBORHOOD: usize = 1;
const PRIOR_COMPLEXITY_WEIGHT: u64 = 48;
const TRANSITION_SCALE: f64 = 1024.0;
const RIBBON_SAMPLE_SPACING_CELLS: f64 = 0.5;
const RIBBON_SLICE_HALF_SPAN: f64 = 0.60;
const RIBBON_MAX_CROSS_SECTION_CELLS: usize = 12;
const RIBBON_MAX_RUN_LENGTH: usize = 4;
const RIBBON_MAX_STATES_PER_SLICE: usize = 16;
const FALLBACK_ITERATIONS: usize = 6;
const FALLBACK_COMPONENT_THRESHOLD: usize = 96;
const PENALTY_OVERLAP_FLIP: u64 = 16_000;
const PENALTY_OVERLAP_SHAPE: u64 = 6_000;
const PENALTY_EMPTY_TRANSITION: u64 = 5_500;
const PENALTY_THICKNESS_DELTA: u64 = 1_400;
const PENALTY_CENTER_SHIFT: u64 = 1_700;
const PENALTY_DISCONNECT: u64 = 18_000;
const Q16_SHIFT: u32 = 16;
const Q16_SCALE: i32 = 1 << Q16_SHIFT;
const Q16_SCALE_U64: u64 = 1_u64 << Q16_SHIFT;
const WIDTH_Q10_SCALE_U64: u64 = 1_024;
const SPEED_SHEATH_START_CPS: f64 = 7.0;
const SPEED_SHEATH_FULL_CPS: f64 = 28.0;
const SPEED_SHEATH_MIN_GAIN: f64 = 0.08;
const SPEED_CORE_START_CPS: f64 = 10.0;
const SPEED_CORE_FULL_CPS: f64 = 34.0;
const SPEED_CORE_MIN_GAIN: f64 = 0.78;
const COMET_NECK_FRACTION: f64 = 0.14;
const COMET_TIP_WIDTH_RATIO: f64 = 0.26;
const COMET_TAPER_EXPONENT: f64 = 1.90;
const COMET_MIN_RESOLVABLE_WIDTH: f64 = 0.26;
const COMET_TIP_ZONE_START: f64 = 0.80;
const COMET_TIP_CAP_MULTIPLIER: f64 = 1.00;
const COMET_MONO_EPSILON_CELLS: f64 = 0.10;
const COMET_CURVATURE_COMPRESS_FACTOR: f64 = 1.05;
const COMET_CURVATURE_KAPPA_CAP: f64 = 1.25;
const COMET_TAPER_WEIGHT: u64 = 3_400;
const COMET_MONO_WEIGHT: u64 = 7_800;
const COMET_TIP_WEIGHT: u64 = 11_800;
const COMET_TRANSVERSE_WEIGHT: u64 = 900;
const CATCH_SALIENCE_DIM_PENALTY: u64 = 100;

const MATRIX_BIT_WEIGHTS: [[u8; 2]; 2] = [[1, 2], [4, 8]];
const OCTANT_BIT_WEIGHTS: [[u8; 2]; 4] = [[1, 2], [4, 8], [16, 32], [64, 128]];

static GLYPH_PROFILES: LazyLock<Vec<GlyphProfile>> = LazyLock::new(build_glyph_profiles);

fn saturating_q16_offset(base: i32, delta: i32) -> i32 {
    (i64::from(base) + i64::from(delta)).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn hash_f64(hasher: &mut DefaultHasher, value: f64) {
    value.to_bits().hash(hasher);
}

fn pose_center(corners: &[Point; 4]) -> Point {
    Point {
        row: (corners[0].row + corners[1].row + corners[2].row + corners[3].row) / 4.0,
        col: (corners[0].col + corners[1].col + corners[2].col + corners[3].col) / 4.0,
    }
}

#[cfg(test)]
fn pose_for_frame(frame: &RenderFrame) -> latent_field::Pose {
    pose_for_corners(&frame.corners, &frame.target_corners)
}

fn pose_for_corners(corners: &[Point; 4], target_corners: &[Point; 4]) -> latent_field::Pose {
    let center = pose_center(corners);
    let width = (target_corners[1].col - target_corners[0].col)
        .abs()
        .max(1.0 / 8.0);
    let height = (target_corners[3].row - target_corners[0].row)
        .abs()
        .max(1.0 / 8.0);

    latent_field::Pose {
        center,
        half_height: height * 0.5,
        half_width: width * 0.5,
    }
}

fn pose_for_step_sample(sample: &RenderStepSample, frame: &RenderFrame) -> latent_field::Pose {
    pose_for_corners(&sample.corners, &frame.target_corners)
}

fn frame_sample_poses(frame: &RenderFrame) -> impl Iterator<Item = latent_field::Pose> + '_ {
    frame
        .step_samples
        .iter()
        .map(|sample| pose_for_step_sample(sample, frame))
}

fn build_block_profile() -> GlyphProfile {
    GlyphProfile {
        glyph: DecodedGlyph::Block,
        occupancy: [1_u8; MICRO_TILE_SAMPLES],
        sample_count: MICRO_TILE_SAMPLES as u16,
        complexity: MICRO_TILE_SAMPLES as u8,
    }
}

fn build_matrix_profile(mask: u8) -> GlyphProfile {
    let mut occupancy = [0_u8; MICRO_TILE_SAMPLES];
    for sample_row in 0..MICRO_H {
        for sample_col in 0..MICRO_W {
            let row_bucket = (sample_row * 2) / MICRO_H;
            let col_bucket = (sample_col * 2) / MICRO_W;
            let bit = MATRIX_BIT_WEIGHTS[row_bucket][col_bucket];
            let index = sample_row * MICRO_W + sample_col;
            occupancy[index] = u8::from(mask & bit != 0);
        }
    }
    let sample_count = occupancy.iter().copied().map(u16::from).sum::<u16>();
    let complexity = mask.count_ones() as u8;
    GlyphProfile {
        glyph: DecodedGlyph::Matrix(mask),
        occupancy,
        sample_count,
        complexity,
    }
}

fn build_octant_profile(mask: u8) -> GlyphProfile {
    let mut occupancy = [0_u8; MICRO_TILE_SAMPLES];
    for sample_row in 0..MICRO_H {
        for sample_col in 0..MICRO_W {
            let row_bucket = (sample_row * 4) / MICRO_H;
            let col_bucket = (sample_col * 2) / MICRO_W;
            let bit = OCTANT_BIT_WEIGHTS[row_bucket][col_bucket];
            let index = sample_row * MICRO_W + sample_col;
            occupancy[index] = u8::from(mask & bit != 0);
        }
    }
    let sample_count = occupancy.iter().copied().map(u16::from).sum::<u16>();
    let complexity = mask.count_ones() as u8;
    GlyphProfile {
        glyph: DecodedGlyph::Octant(mask),
        occupancy,
        sample_count,
        complexity,
    }
}

fn build_glyph_profiles() -> Vec<GlyphProfile> {
    let mut profiles = Vec::with_capacity(1 + 14 + 254);
    profiles.push(build_block_profile());

    for mask in 1_u8..=14_u8 {
        profiles.push(build_matrix_profile(mask));
    }

    for mask in 1_u8..=254_u8 {
        profiles.push(build_octant_profile(mask));
    }

    profiles
}

fn build_shade_profiles(color_levels: u32) -> Vec<ShadeProfile> {
    let capacity = usize::try_from(color_levels).unwrap_or(0);
    let mut shades = Vec::with_capacity(capacity);
    for raw_level in 1..=color_levels {
        let Some(level) = HighlightLevel::try_new(raw_level) else {
            continue;
        };
        shades.push(ShadeProfile {
            level,
            sample_q12: quantized_level_to_sample_q12(level, color_levels),
        });
    }
    shades
}

fn quantized_level_to_sample_q12(level: HighlightLevel, color_levels: u32) -> u16 {
    if color_levels == 0 {
        return 0;
    }
    let numerator = u64::from(level.value()).saturating_mul(4095_u64);
    let denominator = u64::from(color_levels).max(1);
    (numerator / denominator).min(u64::from(u16::MAX)) as u16
}

impl LocalCellProfile {
    fn new(glyph: GlyphProfile, shade: ShadeProfile) -> Self {
        Self {
            state: DecodedCellState {
                glyph: glyph.glyph,
                level: shade.level,
            },
            glyph,
            shade,
        }
    }
}

fn nearest_shade_profile_index(shades: &[ShadeProfile], alpha_q12: u16) -> Option<usize> {
    shades
        .iter()
        .enumerate()
        .min_by_key(|(_, shade)| shade.sample_q12.abs_diff(alpha_q12))
        .map(|(index, _)| index)
}

fn previous_shade_profile_index(
    shades: &[ShadeProfile],
    previous: Option<DecodedCellState>,
    glyph: DecodedGlyph,
) -> Option<usize> {
    let DecodedCellState {
        glyph: previous_glyph,
        level,
    } = previous?;
    if previous_glyph != glyph {
        return None;
    }

    let index = usize::try_from(level.value().saturating_sub(1)).ok()?;
    (index < shades.len()).then_some(index)
}

fn shade_profile_indices_for_glyph(
    shades: &[ShadeProfile],
    alpha_q12: u16,
    previous: Option<DecodedCellState>,
    glyph: DecodedGlyph,
) -> Vec<usize> {
    let Some(nearest_index) = nearest_shade_profile_index(shades, alpha_q12) else {
        return Vec::new();
    };
    let start = nearest_index.saturating_sub(SHADE_PROFILE_NEIGHBORHOOD);
    let end = nearest_index
        .saturating_add(SHADE_PROFILE_NEIGHBORHOOD)
        .min(shades.len().saturating_sub(1));
    let mut indices = (start..=end).collect::<Vec<_>>();

    // Surprising: temporal stability cannot suppress shade flicker if local fitting only offers
    // one quantized level per glyph, so we keep a tiny neighboring shade ladder alive here.
    if let Some(previous_index) = previous_shade_profile_index(shades, previous, glyph) {
        indices.push(previous_index);
    }

    indices.sort_unstable();
    indices.dedup();
    indices
}

fn residual_cost(patch: &MicroTile, profile: LocalCellProfile) -> u64 {
    patch
        .samples_q12
        .iter()
        .copied()
        .zip(profile.glyph.occupancy.iter().copied())
        .map(|(patch_sample, coverage)| {
            let expected = if coverage == 0 {
                0_i64
            } else {
                i64::from(profile.shade.sample_q12)
            };
            let delta = i64::from(patch_sample) - expected;
            (delta * delta) as u64
        })
        .sum::<u64>()
}

fn state_sort_key(state: Option<DecodedCellState>) -> u32 {
    match state {
        None => 0,
        Some(DecodedCellState {
            glyph: DecodedGlyph::Block,
            level,
        }) => 100_000 + level.value(),
        Some(DecodedCellState {
            glyph: DecodedGlyph::Matrix(mask),
            level,
        }) => 200_000 + u32::from(mask) * 256 + level.value(),
        Some(DecodedCellState {
            glyph: DecodedGlyph::Octant(mask),
            level,
        }) => 300_000 + u32::from(mask) * 256 + level.value(),
    }
}

fn temporal_transition_distance(
    previous: Option<DecodedCellState>,
    next: Option<DecodedCellState>,
) -> u32 {
    match (previous, next) {
        (None, None) => 0,
        (None, Some(_)) | (Some(_), None) => 48,
        (Some(prev), Some(next_state)) if prev == next_state => 0,
        (Some(prev), Some(next_state)) if prev.glyph == next_state.glyph => {
            prev.level.abs_diff(next_state.level) * 2
        }
        (Some(prev), Some(next_state)) => {
            24_u32.saturating_add(prev.level.abs_diff(next_state.level))
        }
    }
}

fn temporal_cost(
    previous: Option<DecodedCellState>,
    next: Option<DecodedCellState>,
    age: AgeMoment,
    temporal_stability_weight: f64,
) -> u64 {
    let total_mass = age.total_mass_q12;
    let head_ratio = if total_mass == 0 {
        1.0
    } else {
        (age.recent_mass_q12 as f64 / total_mass as f64).clamp(0.0, 1.0)
    };

    let adaptive_weight = if temporal_stability_weight.is_finite() {
        temporal_stability_weight.max(0.0) * (1.0 - head_ratio)
    } else {
        0.0
    };

    let transition = temporal_transition_distance(previous, next) as f64;
    (adaptive_weight * transition * TRANSITION_SCALE)
        .round()
        .max(0.0) as u64
}

fn candidate_cmp(
    lhs: CellCandidate,
    rhs: CellCandidate,
    previous: Option<DecodedCellState>,
) -> Ordering {
    lhs.unary_cost
        .cmp(&rhs.unary_cost)
        .then_with(|| u8::from(lhs.state != previous).cmp(&u8::from(rhs.state != previous)))
        .then_with(|| state_sort_key(lhs.state).cmp(&state_sort_key(rhs.state)))
}

fn cell_candidates_for_patch(
    patch: MicroTile,
    age: AgeMoment,
    previous: Option<DecodedCellState>,
    shade_profiles: &[ShadeProfile],
    temporal_stability_weight: f64,
    top_k: usize,
) -> Vec<CellCandidate> {
    let empty_residual = patch
        .samples_q12
        .iter()
        .copied()
        .map(|sample| {
            let value = u64::from(sample);
            value.saturating_mul(value)
        })
        .sum::<u64>();

    let empty_candidate = CellCandidate {
        state: None,
        unary_cost: empty_residual.saturating_add(temporal_cost(
            previous,
            None,
            age,
            temporal_stability_weight,
        )),
    };
    if patch.max_sample_q12() < MIN_VISIBLE_SAMPLE_Q12 {
        return vec![empty_candidate];
    }
    let mut non_empty = Vec::<CellCandidate>::new();

    for glyph in GLYPH_PROFILES.iter().copied() {
        if glyph.sample_count == 0 || shade_profiles.is_empty() {
            continue;
        }

        let dot = patch
            .samples_q12
            .iter()
            .copied()
            .zip(glyph.occupancy.iter().copied())
            .map(|(sample, coverage)| {
                if coverage == 0 {
                    0_u64
                } else {
                    u64::from(sample)
                }
            })
            .sum::<u64>();
        if dot == 0 {
            continue;
        }

        let alpha_q12 = (dot / u64::from(glyph.sample_count)).min(u64::from(u16::MAX)) as u16;
        if alpha_q12 < MIN_VISIBLE_SAMPLE_Q12 {
            continue;
        }

        for shade_index in
            shade_profile_indices_for_glyph(shade_profiles, alpha_q12, previous, glyph.glyph)
        {
            let Some(shade) = shade_profiles.get(shade_index).copied() else {
                continue;
            };
            let profile = LocalCellProfile::new(glyph, shade);
            let residual = residual_cost(&patch, profile);
            let prior = u64::from(profile.glyph.complexity)
                .saturating_mul(u64::from(profile.glyph.complexity))
                .saturating_mul(PRIOR_COMPLEXITY_WEIGHT);
            let state = Some(profile.state);
            let total_cost = residual.saturating_add(prior).saturating_add(temporal_cost(
                previous,
                state,
                age,
                temporal_stability_weight,
            ));
            non_empty.push(CellCandidate {
                state,
                unary_cost: total_cost,
            });
        }
    }

    non_empty.sort_by(|lhs, rhs| candidate_cmp(*lhs, *rhs, previous));
    non_empty.dedup_by_key(|candidate| candidate.state);

    let keep_non_empty = top_k.saturating_sub(1);
    let mut kept = Vec::with_capacity(1 + keep_non_empty);
    kept.push(empty_candidate);
    kept.extend(non_empty.into_iter().take(keep_non_empty));
    kept.sort_by(|lhs, rhs| candidate_cmp(*lhs, *rhs, previous));
    kept
}

fn build_cell_candidates(
    compiled: &BTreeMap<(i64, i64), CompiledCell>,
    previous_cells: &BTreeMap<(i64, i64), DecodedCellState>,
    color_levels: u32,
    temporal_stability_weight: f64,
    top_k: usize,
) -> BTreeMap<(i64, i64), Vec<CellCandidate>> {
    let shade_profiles = build_shade_profiles(color_levels);
    let mut candidates = BTreeMap::<(i64, i64), Vec<CellCandidate>>::new();

    for (key, compiled_cell) in compiled {
        let previous = previous_cells.get(key).copied();
        let per_cell = cell_candidates_for_patch(
            compiled_cell.tile,
            compiled_cell.age,
            previous,
            &shade_profiles,
            temporal_stability_weight,
            top_k,
        );
        candidates.insert(*key, per_cell);
    }

    for (key, previous) in previous_cells {
        if candidates.contains_key(key) {
            continue;
        }
        let per_cell = cell_candidates_for_patch(
            MicroTile::default(),
            AgeMoment::default(),
            Some(*previous),
            &shade_profiles,
            temporal_stability_weight,
            top_k,
        );
        candidates.insert(*key, per_cell);
    }
    candidates
}

fn decode_locally(
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
) -> BTreeMap<(i64, i64), DecodedCellState> {
    let mut next_cells = BTreeMap::<(i64, i64), DecodedCellState>::new();
    for (coord, candidates) in cell_candidates {
        if let Some(state) = candidates.first().and_then(|candidate| candidate.state) {
            next_cells.insert(*coord, state);
        }
    }
    next_cells
}

fn non_empty_candidates(candidates: &[CellCandidate]) -> Vec<CellCandidate> {
    candidates
        .iter()
        .copied()
        .filter(|candidate| candidate.state.is_some())
        .collect::<Vec<_>>()
}

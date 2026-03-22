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
        NonZeroU32::new(clamped).map_or(Self(NonZeroU32::MIN), Self)
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
        let raw_index = usize::try_from(self.value()).map_or(usize::MAX, |index| index);
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
            Self::Braille(index) => BRAILLE_GLYPHS
                .get(index.saturating_sub(1) as usize)
                .map_or("", |value| value.as_ref()),
        }
    }
}

// cache boxed braille strings once so `Glyph::Braille` can hand extmark writes a
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
    latent_cache: LatentFieldCache,
    center_history: VecDeque<CenterPathSample>,
    previous_cells: BTreeMap<(i64, i64), DecodedCellState>,
    compiled_cache: CompiledFieldCache,
    // Production planner truth lives in `latent_cache`; tests keep a mirrored slice log for
    // assertions over staged metadata without forcing the runtime to retain the trail twice.
    #[cfg(test)]
    history: VecDeque<DepositedSlice>,
}

impl PlannerState {
    pub(crate) const fn step_index(&self) -> StepIndex {
        self.step_index
    }

    pub(crate) const fn history_revision(&self) -> u64 {
        self.latent_cache.revision()
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
}

#[derive(Clone, Debug)]
pub(crate) struct CompiledPlannerFrame {
    next_state: PlannerState,
    compiled: Arc<BTreeMap<(i64, i64), CompiledCell>>,
}

#[derive(Debug, Default)]
struct CompiledFieldCache {
    latest_step: Option<StepIndex>,
    latent_revision: u64,
    field: Arc<BTreeMap<(i64, i64), CompiledCell>>,
    scratch: latent_field::CompileScratch,
}

impl Clone for CompiledFieldCache {
    fn clone(&self) -> Self {
        Self {
            latest_step: self.latest_step,
            latent_revision: self.latent_revision,
            field: Arc::clone(&self.field),
            scratch: latent_field::CompileScratch::default(),
        }
    }
}

impl PartialEq for CompiledFieldCache {
    fn eq(&self, other: &Self) -> bool {
        self.latest_step == other.latest_step
            && self.latent_revision == other.latent_revision
            && self.field == other.field
    }
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
    sample_count: usize,
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

#[derive(Clone, Copy, Debug)]
struct GlyphBucketLayout {
    matrix_bucket_for_sample: [u8; MICRO_TILE_SAMPLES],
    octant_bucket_for_sample: [u8; MICRO_TILE_SAMPLES],
    matrix_sample_counts_by_mask: [u8; MATRIX_MASK_LIMIT],
    octant_sample_counts_by_mask: [u8; OCTANT_MASK_LIMIT],
}

#[derive(Clone, Copy, Debug, Default)]
struct PatchCandidateBasis {
    empty_residual: u64,
    total_mass: u64,
    matrix_bucket_sums: [u64; MATRIX_BUCKET_COUNT],
    octant_bucket_sums: [u64; OCTANT_BUCKET_COUNT],
}

#[derive(Clone, Copy, Debug, Default)]
struct ShadeProfileIndexSet {
    indices: [usize; MAX_SHADE_PROFILE_CANDIDATES],
    len: usize,
}

impl ShadeProfileIndexSet {
    fn push_unique(&mut self, index: usize) {
        if self.indices[..self.len].contains(&index) {
            return;
        }
        if self.len >= self.indices.len() {
            return;
        }
        self.indices[self.len] = index;
        self.len += 1;
    }

    fn as_slice(&self) -> &[usize] {
        &self.indices[..self.len]
    }
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

    #[cfg(test)]
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
        Self::try_new(min_q16, max_q16).map_or(
            Self {
                min_q16: max_q16,
                max_q16: min_q16,
            },
            |span| span,
        )
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
const MATRIX_BUCKET_COUNT: usize = 4;
const MATRIX_MASK_LIMIT: usize = 1 << MATRIX_BUCKET_COUNT;
const OCTANT_BUCKET_COUNT: usize = 8;
const OCTANT_MASK_LIMIT: usize = 1 << OCTANT_BUCKET_COUNT;
const MIN_VISIBLE_SAMPLE_Q12: u16 = 6;
const SHADE_PROFILE_NEIGHBORHOOD: usize = 1;
const MAX_SHADE_PROFILE_CANDIDATES: usize = SHADE_PROFILE_NEIGHBORHOOD * 2 + 2;
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

#[cfg(test)]
const MATRIX_BIT_WEIGHTS: [[u8; 2]; 2] = [[1, 2], [4, 8]];
#[cfg(test)]
const OCTANT_BIT_WEIGHTS: [[u8; 2]; 4] = [[1, 2], [4, 8], [16, 32], [64, 128]];

static GLYPH_BUCKET_LAYOUT: LazyLock<GlyphBucketLayout> = LazyLock::new(build_glyph_bucket_layout);

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

fn build_glyph_bucket_layout() -> GlyphBucketLayout {
    let mut matrix_bucket_for_sample = [0_u8; MICRO_TILE_SAMPLES];
    let mut octant_bucket_for_sample = [0_u8; MICRO_TILE_SAMPLES];
    let mut matrix_bucket_sample_counts = [0_u8; MATRIX_BUCKET_COUNT];
    let mut octant_bucket_sample_counts = [0_u8; OCTANT_BUCKET_COUNT];

    for sample_row in 0..MICRO_H {
        for sample_col in 0..MICRO_W {
            let index = sample_row * MICRO_W + sample_col;

            let matrix_row_bucket = (sample_row * 2) / MICRO_H;
            let matrix_col_bucket = (sample_col * 2) / MICRO_W;
            let matrix_bucket = matrix_row_bucket * 2 + matrix_col_bucket;
            matrix_bucket_for_sample[index] = u8::try_from(matrix_bucket).unwrap_or(u8::MAX);
            matrix_bucket_sample_counts[matrix_bucket] =
                matrix_bucket_sample_counts[matrix_bucket].saturating_add(1);

            let octant_row_bucket = (sample_row * 4) / MICRO_H;
            let octant_col_bucket = (sample_col * 2) / MICRO_W;
            let octant_bucket = octant_row_bucket * 2 + octant_col_bucket;
            octant_bucket_for_sample[index] = u8::try_from(octant_bucket).unwrap_or(u8::MAX);
            octant_bucket_sample_counts[octant_bucket] =
                octant_bucket_sample_counts[octant_bucket].saturating_add(1);
        }
    }

    GlyphBucketLayout {
        matrix_bucket_for_sample,
        octant_bucket_for_sample,
        matrix_sample_counts_by_mask: build_mask_sample_counts(&matrix_bucket_sample_counts),
        octant_sample_counts_by_mask: build_mask_sample_counts(&octant_bucket_sample_counts),
    }
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

impl GlyphProfile {
    fn block() -> Self {
        Self {
            glyph: DecodedGlyph::Block,
            sample_count: MICRO_TILE_SAMPLES,
            complexity: u8::try_from(MICRO_TILE_SAMPLES).unwrap_or(u8::MAX),
        }
    }

    fn matrix(mask: u8, sample_count: usize) -> Self {
        Self {
            glyph: DecodedGlyph::Matrix(mask),
            sample_count,
            complexity: mask.count_ones() as u8,
        }
    }

    fn octant(mask: u8, sample_count: usize) -> Self {
        Self {
            glyph: DecodedGlyph::Octant(mask),
            sample_count,
            complexity: mask.count_ones() as u8,
        }
    }
}

impl GlyphBucketLayout {
    fn matrix_sample_count(&self, mask: u8) -> usize {
        usize::from(self.matrix_sample_counts_by_mask[usize::from(mask)])
    }

    fn octant_sample_count(&self, mask: u8) -> usize {
        usize::from(self.octant_sample_counts_by_mask[usize::from(mask)])
    }
}

impl PatchCandidateBasis {
    fn from_patch(patch: MicroTile) -> Self {
        let layout = &*GLYPH_BUCKET_LAYOUT;
        let mut basis = Self::default();

        // All glyph families are unions of these micro-buckets, so one patch pass recovers the
        // exact per-glyph dot products that the old occupied-sample scan computed.
        for (index, sample_q12) in patch.samples_q12.iter().copied().enumerate() {
            let value = u64::from(sample_q12);
            basis.empty_residual = basis
                .empty_residual
                .saturating_add(value.saturating_mul(value));
            basis.total_mass = basis.total_mass.saturating_add(value);

            let matrix_bucket = usize::from(layout.matrix_bucket_for_sample[index]);
            basis.matrix_bucket_sums[matrix_bucket] =
                basis.matrix_bucket_sums[matrix_bucket].saturating_add(value);

            let octant_bucket = usize::from(layout.octant_bucket_for_sample[index]);
            basis.octant_bucket_sums[octant_bucket] =
                basis.octant_bucket_sums[octant_bucket].saturating_add(value);
        }

        basis
    }
}

fn build_mask_sample_counts<const BUCKETS: usize, const MASK_LIMIT: usize>(
    bucket_sample_counts: &[u8; BUCKETS],
) -> [u8; MASK_LIMIT] {
    let mut sample_counts = [0_u8; MASK_LIMIT];
    let mut mask = 1_usize;
    while mask < MASK_LIMIT {
        let lsb_index = mask.trailing_zeros() as usize;
        let previous = mask & (mask - 1);
        sample_counts[mask] =
            sample_counts[previous].saturating_add(bucket_sample_counts[lsb_index]);
        mask += 1;
    }
    sample_counts
}

fn build_subset_sums<const BUCKETS: usize, const MASK_LIMIT: usize>(
    bucket_sums: &[u64; BUCKETS],
) -> [u64; MASK_LIMIT] {
    let mut subset_sums = [0_u64; MASK_LIMIT];
    let mut mask = 1_usize;
    while mask < MASK_LIMIT {
        let lsb_index = mask.trailing_zeros() as usize;
        let previous = mask & (mask - 1);
        subset_sums[mask] = subset_sums[previous].saturating_add(bucket_sums[lsb_index]);
        mask += 1;
    }
    subset_sums
}

fn nearest_shade_profile_index(shades: &[ShadeProfile], alpha_q12: u16) -> Option<usize> {
    let shade_count = shades.len();
    if shade_count == 0 {
        return None;
    }

    let shade_count_u64 = u64::try_from(shade_count).unwrap_or(u64::MAX);
    let rounded_level = u64::from(alpha_q12)
        .saturating_mul(shade_count_u64)
        .saturating_add(2047)
        .saturating_div(4095)
        .clamp(1, shade_count_u64);
    let candidate_index = usize::try_from(rounded_level.saturating_sub(1)).unwrap_or(0);
    let start = candidate_index.saturating_sub(1);
    let end = candidate_index
        .saturating_add(1)
        .min(shade_count.saturating_sub(1));

    (start..=end).min_by_key(|index| shades[*index].sample_q12.abs_diff(alpha_q12))
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
) -> ShadeProfileIndexSet {
    let Some(nearest_index) = nearest_shade_profile_index(shades, alpha_q12) else {
        return ShadeProfileIndexSet::default();
    };
    let start = nearest_index.saturating_sub(SHADE_PROFILE_NEIGHBORHOOD);
    let end = nearest_index
        .saturating_add(SHADE_PROFILE_NEIGHBORHOOD)
        .min(shades.len().saturating_sub(1));
    let previous_index = previous_shade_profile_index(shades, previous, glyph);
    let mut indices = ShadeProfileIndexSet::default();

    if let Some(previous_index) = previous_index.filter(|index| *index < start) {
        indices.push_unique(previous_index);
    }

    for index in start..=end {
        indices.push_unique(index);
    }

    // Surprising: temporal stability cannot suppress shade flicker if local fitting only offers
    // one quantized level per glyph, so we keep a tiny neighboring shade ladder alive here.
    if let Some(previous_index) = previous_index.filter(|index| *index > end) {
        indices.push_unique(previous_index);
    }
    indices
}

fn residual_cost(empty_residual: u64, dot: u64, profile: LocalCellProfile) -> u64 {
    let shade = i128::from(profile.shade.sample_q12);
    let occupied_mass = i128::try_from(profile.glyph.sample_count).unwrap_or(i128::MAX);
    let residual = i128::from(empty_residual) + occupied_mass * shade * shade
        - 2_i128 * shade * i128::from(dot);
    debug_assert!(residual >= 0);
    residual.max(0) as u64
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

fn insert_best_non_empty_candidate(
    candidates: &mut Vec<CellCandidate>,
    candidate: CellCandidate,
    previous: Option<DecodedCellState>,
    keep_non_empty: usize,
) {
    if keep_non_empty == 0 {
        return;
    }

    let insert_at = candidates.partition_point(|existing| {
        candidate_cmp(*existing, candidate, previous) != Ordering::Greater
    });
    if candidates.len() >= keep_non_empty && insert_at == candidates.len() {
        return;
    }

    candidates.insert(insert_at, candidate);
    if candidates.len() > keep_non_empty {
        let _ = candidates.pop();
    }
}

struct NonEmptyCandidateContext<'a> {
    empty_residual: u64,
    age: AgeMoment,
    previous: Option<DecodedCellState>,
    shade_profiles: &'a [ShadeProfile],
    temporal_stability_weight: f64,
    keep_non_empty: usize,
}

fn evaluate_non_empty_glyph_candidate(
    candidates: &mut Vec<CellCandidate>,
    glyph: GlyphProfile,
    dot: u64,
    context: &NonEmptyCandidateContext<'_>,
) {
    if glyph.sample_count == 0 || dot == 0 {
        return;
    }

    let alpha_q12 = (dot / u64::try_from(glyph.sample_count).unwrap_or(u64::MAX))
        .min(u64::from(u16::MAX)) as u16;
    if alpha_q12 < MIN_VISIBLE_SAMPLE_Q12 {
        return;
    }

    let prior = u64::from(glyph.complexity)
        .saturating_mul(u64::from(glyph.complexity))
        .saturating_mul(PRIOR_COMPLEXITY_WEIGHT);
    let shade_indices = shade_profile_indices_for_glyph(
        context.shade_profiles,
        alpha_q12,
        context.previous,
        glyph.glyph,
    );
    for shade_index in shade_indices.as_slice().iter().copied() {
        let shade = context.shade_profiles[shade_index];
        let profile = LocalCellProfile::new(glyph, shade);
        let residual = residual_cost(context.empty_residual, dot, profile);
        let state = Some(profile.state);
        let total_cost = residual.saturating_add(prior).saturating_add(temporal_cost(
            context.previous,
            state,
            context.age,
            context.temporal_stability_weight,
        ));
        insert_best_non_empty_candidate(
            candidates,
            CellCandidate {
                state,
                unary_cost: total_cost,
            },
            context.previous,
            context.keep_non_empty,
        );
    }
}

fn cell_candidates_for_patch(
    patch: MicroTile,
    age: AgeMoment,
    previous: Option<DecodedCellState>,
    shade_profiles: &[ShadeProfile],
    temporal_stability_weight: f64,
    top_k: usize,
) -> Vec<CellCandidate> {
    let patch_basis = PatchCandidateBasis::from_patch(patch);
    let empty_residual = patch_basis.empty_residual;

    let empty_candidate = CellCandidate {
        state: None,
        unary_cost: empty_residual.saturating_add(temporal_cost(
            previous,
            None,
            age,
            temporal_stability_weight,
        )),
    };
    let keep_non_empty = top_k.saturating_sub(1);
    if patch.max_sample_q12() < MIN_VISIBLE_SAMPLE_Q12
        || keep_non_empty == 0
        || shade_profiles.is_empty()
    {
        return vec![empty_candidate];
    }
    let mut non_empty = Vec::<CellCandidate>::with_capacity(keep_non_empty);
    let non_empty_context = NonEmptyCandidateContext {
        empty_residual,
        age,
        previous,
        shade_profiles,
        temporal_stability_weight,
        keep_non_empty,
    };

    evaluate_non_empty_glyph_candidate(
        &mut non_empty,
        GlyphProfile::block(),
        patch_basis.total_mass,
        &non_empty_context,
    );

    let layout = &*GLYPH_BUCKET_LAYOUT;
    let matrix_dots: [u64; MATRIX_MASK_LIMIT] = build_subset_sums(&patch_basis.matrix_bucket_sums);
    for mask in 1_u8..=14_u8 {
        evaluate_non_empty_glyph_candidate(
            &mut non_empty,
            GlyphProfile::matrix(mask, layout.matrix_sample_count(mask)),
            matrix_dots[usize::from(mask)],
            &non_empty_context,
        );
    }

    let octant_dots: [u64; OCTANT_MASK_LIMIT] = build_subset_sums(&patch_basis.octant_bucket_sums);
    for mask in 1_u8..=254_u8 {
        evaluate_non_empty_glyph_candidate(
            &mut non_empty,
            GlyphProfile::octant(mask, layout.octant_sample_count(mask)),
            octant_dots[usize::from(mask)],
            &non_empty_context,
        );
    }

    let mut kept = Vec::with_capacity(1 + keep_non_empty);
    kept.push(empty_candidate);
    kept.extend(non_empty);
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

#[cfg(test)]
mod candidate_generation_complexity {
    use super::*;

    fn varied_patch() -> MicroTile {
        let mut patch = MicroTile::default();
        for sample_row in 0..MICRO_H {
            for sample_col in 0..MICRO_W {
                let index = sample_row * MICRO_W + sample_col;
                let base = ((sample_row * 521 + sample_col * 193 + index * 17) % 4096) as u16;
                patch.samples_q12[index] = if (sample_row + sample_col) % 5 == 0 {
                    base / 16
                } else {
                    base
                };
            }
        }
        patch
    }

    fn legacy_glyph_dot(patch: MicroTile, glyph: GlyphProfile) -> u64 {
        match glyph.glyph {
            DecodedGlyph::Block => patch
                .samples_q12
                .iter()
                .copied()
                .map(u64::from)
                .sum::<u64>(),
            DecodedGlyph::Matrix(mask) => {
                let mut dot = 0_u64;
                for sample_row in 0..MICRO_H {
                    for sample_col in 0..MICRO_W {
                        let row_bucket = (sample_row * 2) / MICRO_H;
                        let col_bucket = (sample_col * 2) / MICRO_W;
                        let bit = MATRIX_BIT_WEIGHTS[row_bucket][col_bucket];
                        if mask & bit != 0 {
                            let index = sample_row * MICRO_W + sample_col;
                            dot = dot.saturating_add(u64::from(patch.samples_q12[index]));
                        }
                    }
                }
                dot
            }
            DecodedGlyph::Octant(mask) => {
                let mut dot = 0_u64;
                for sample_row in 0..MICRO_H {
                    for sample_col in 0..MICRO_W {
                        let row_bucket = (sample_row * 4) / MICRO_H;
                        let col_bucket = (sample_col * 2) / MICRO_W;
                        let bit = OCTANT_BIT_WEIGHTS[row_bucket][col_bucket];
                        if mask & bit != 0 {
                            let index = sample_row * MICRO_W + sample_col;
                            dot = dot.saturating_add(u64::from(patch.samples_q12[index]));
                        }
                    }
                }
                dot
            }
        }
    }

    fn legacy_cell_candidates_for_patch(
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
        let keep_non_empty = top_k.saturating_sub(1);
        if patch.max_sample_q12() < MIN_VISIBLE_SAMPLE_Q12
            || keep_non_empty == 0
            || shade_profiles.is_empty()
        {
            return vec![empty_candidate];
        }

        let layout = &*GLYPH_BUCKET_LAYOUT;
        let mut non_empty = Vec::<CellCandidate>::with_capacity(keep_non_empty);
        let non_empty_context = NonEmptyCandidateContext {
            empty_residual,
            age,
            previous,
            shade_profiles,
            temporal_stability_weight,
            keep_non_empty,
        };
        let mut glyphs = Vec::with_capacity(1 + 14 + 254);
        glyphs.push(GlyphProfile::block());
        for mask in 1_u8..=14_u8 {
            glyphs.push(GlyphProfile::matrix(mask, layout.matrix_sample_count(mask)));
        }
        for mask in 1_u8..=254_u8 {
            glyphs.push(GlyphProfile::octant(mask, layout.octant_sample_count(mask)));
        }

        for glyph in glyphs {
            let dot = legacy_glyph_dot(patch, glyph);
            evaluate_non_empty_glyph_candidate(&mut non_empty, glyph, dot, &non_empty_context);
        }

        let mut kept = Vec::with_capacity(1 + keep_non_empty);
        kept.push(empty_candidate);
        kept.extend(non_empty);
        kept.sort_by(|lhs, rhs| candidate_cmp(*lhs, *rhs, previous));
        kept
    }

    #[test]
    fn patch_candidate_basis_matches_legacy_mask_dots_for_all_families() {
        let patch = varied_patch();
        let patch_basis = PatchCandidateBasis::from_patch(patch);
        let matrix_dots: [u64; MATRIX_MASK_LIMIT] =
            build_subset_sums(&patch_basis.matrix_bucket_sums);
        let octant_dots: [u64; OCTANT_MASK_LIMIT] =
            build_subset_sums(&patch_basis.octant_bucket_sums);
        let layout = &*GLYPH_BUCKET_LAYOUT;

        assert_eq!(
            patch_basis.total_mass,
            legacy_glyph_dot(patch, GlyphProfile::block())
        );

        for mask in 1_u8..=14_u8 {
            let glyph = GlyphProfile::matrix(mask, layout.matrix_sample_count(mask));
            assert_eq!(
                matrix_dots[usize::from(mask)],
                legacy_glyph_dot(patch, glyph)
            );
        }

        for mask in 1_u8..=254_u8 {
            let glyph = GlyphProfile::octant(mask, layout.octant_sample_count(mask));
            assert_eq!(
                octant_dots[usize::from(mask)],
                legacy_glyph_dot(patch, glyph)
            );
        }
    }

    #[test]
    fn cell_candidates_for_patch_matches_legacy_profile_scan_with_previous_state() {
        let patch = varied_patch();
        let age = AgeMoment {
            total_mass_q12: 4095,
            recent_mass_q12: 1024,
        };
        let previous = Some(DecodedCellState {
            glyph: DecodedGlyph::Octant(18),
            level: HighlightLevel::from_raw_clamped(7),
        });
        let shade_profiles = build_shade_profiles(16);

        let current = cell_candidates_for_patch(patch, age, previous, &shade_profiles, 0.35, 5);
        let legacy =
            legacy_cell_candidates_for_patch(patch, age, previous, &shade_profiles, 0.35, 5);

        assert_eq!(current, legacy);
    }
}

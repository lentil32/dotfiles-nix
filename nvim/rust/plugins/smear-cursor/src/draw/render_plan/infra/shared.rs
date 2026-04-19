use crate::core::types::ArcLenQ16;
use crate::core::types::StepIndex;
use crate::core::types::StrokeId;
use crate::draw::BRAILLE_CODE_MIN;
use crate::types::CursorCellShape;
use crate::types::Point;
use crate::types::RenderFrame;
use crate::types::RenderStepSample;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::LazyLock;

use super::super::latent_field::BorrowedCellRowsScratch;
use super::super::latent_field::CellRows;
use super::super::latent_field::CompiledCell;
#[cfg(test)]
use super::super::latent_field::DepositedSlice;
use super::super::latent_field::LatentFieldCache;
use super::super::local_envelope::SliceSearchBounds;

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

// Cache boxed braille strings once so `Glyph::Braille` can hand extmark writes a
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
    pub(crate) shape: CursorCellShape,
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
pub(crate) struct DecodedCellState {
    pub(crate) glyph: DecodedGlyph,
    pub(crate) level: HighlightLevel,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum DecodedGlyph {
    Block,
    Matrix(u8),
    Octant(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ShadeProfile {
    pub(crate) level: HighlightLevel,
    pub(crate) sample_q12: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CellCandidate {
    pub(crate) state: Option<DecodedCellState>,
    pub(crate) unary_cost: u64,
}

type SharedLatentFieldCache = Arc<LatentFieldCache>;
type SharedCenterHistory = Arc<VecDeque<CenterPathSample>>;
type SharedPreviousCells = Arc<BTreeMap<(i64, i64), DecodedCellState>>;

#[derive(Debug, Default)]
pub(crate) struct PlannerState {
    pub(in crate::draw::render_plan) step_index: StepIndex,
    pub(in crate::draw::render_plan) arc_len_q16: ArcLenQ16,
    pub(in crate::draw::render_plan) last_trail_stroke_id: Option<StrokeId>,
    pub(in crate::draw::render_plan) last_pose: Option<super::super::latent_field::Pose>,
    pub(in crate::draw::render_plan) latent_cache: SharedLatentFieldCache,
    pub(in crate::draw::render_plan) center_history: SharedCenterHistory,
    pub(in crate::draw::render_plan) previous_cells: SharedPreviousCells,
    pub(in crate::draw::render_plan) compiled_cache: CompiledFieldCache,
    pub(in crate::draw::render_plan) decode_scratch: PlannerDecodeScratch,
    pub(in crate::draw::render_plan) sweep_scratch:
        super::super::latent_field::SweepMaterializeScratch,
    // Production planner truth lives in `latent_cache`; tests keep a mirrored slice log for
    // assertions over staged metadata without forcing the runtime to retain the trail twice.
    #[cfg(test)]
    pub(in crate::draw::render_plan) history: VecDeque<DepositedSlice>,
}

impl PlannerState {
    pub(crate) const fn step_index(&self) -> StepIndex {
        self.step_index
    }

    pub(crate) fn history_revision(&self) -> u64 {
        self.latent_cache.revision()
    }

    pub(in crate::draw::render_plan) fn latent_cache_mut(&mut self) -> &mut LatentFieldCache {
        Arc::make_mut(&mut self.latent_cache)
    }

    pub(in crate::draw::render_plan) fn center_history_mut(
        &mut self,
    ) -> &mut VecDeque<CenterPathSample> {
        Arc::make_mut(&mut self.center_history)
    }
}

impl Clone for PlannerState {
    fn clone(&self) -> Self {
        Self {
            step_index: self.step_index,
            arc_len_q16: self.arc_len_q16,
            last_trail_stroke_id: self.last_trail_stroke_id,
            last_pose: self.last_pose,
            latent_cache: Arc::clone(&self.latent_cache),
            center_history: Arc::clone(&self.center_history),
            previous_cells: Arc::clone(&self.previous_cells),
            compiled_cache: self.compiled_cache.clone(),
            decode_scratch: PlannerDecodeScratch::default(),
            sweep_scratch: super::super::latent_field::SweepMaterializeScratch::default(),
            #[cfg(test)]
            history: self.history.clone(),
        }
    }
}

impl PartialEq for PlannerState {
    fn eq(&self, other: &Self) -> bool {
        self.step_index == other.step_index
            && self.arc_len_q16 == other.arc_len_q16
            && self.last_trail_stroke_id == other.last_trail_stroke_id
            && self.last_pose == other.last_pose
            && self.latent_cache == other.latent_cache
            && self.center_history == other.center_history
            && self.previous_cells == other.previous_cells
            && self.compiled_cache == other.compiled_cache
            && {
                #[cfg(test)]
                {
                    self.history == other.history
                }
                #[cfg(not(test))]
                {
                    true
                }
            }
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
pub(in crate::draw::render_plan) struct CompiledPlannerFrame {
    pub(in crate::draw::render_plan) next_state: PlannerState,
    pub(in crate::draw::render_plan) compiled: Arc<CompiledField>,
    pub(in crate::draw::render_plan) query_bounds: Option<SliceSearchBounds>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::draw::render_plan) enum CompiledField {
    Reference(BTreeMap<(i64, i64), CompiledCell>),
    Rows(CellRows<CompiledCell>),
}

impl CompiledField {
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Reference(cells) => cells.len(),
            Self::Rows(cells) => cells.len(),
        }
    }

    #[cfg(test)]
    pub(in crate::draw::render_plan) fn to_btree_map(&self) -> BTreeMap<(i64, i64), CompiledCell> {
        match self {
            Self::Reference(cells) => cells.clone(),
            Self::Rows(cells) => cells.iter().map(|(coord, value)| (coord, *value)).collect(),
        }
    }
}

impl Default for CompiledField {
    fn default() -> Self {
        Self::Reference(BTreeMap::new())
    }
}

#[derive(Debug, Default)]
pub(crate) struct CompiledFieldCache {
    pub(crate) latest_step: Option<StepIndex>,
    pub(crate) latent_revision: u64,
    pub(crate) query_bounds: Option<SliceSearchBounds>,
    pub(crate) field: Arc<CompiledField>,
    pub(crate) scratch: super::super::latent_field::CompileScratch,
}

impl Clone for CompiledFieldCache {
    fn clone(&self) -> Self {
        Self {
            latest_step: self.latest_step,
            latent_revision: self.latent_revision,
            query_bounds: self.query_bounds,
            field: Arc::clone(&self.field),
            scratch: super::super::latent_field::CompileScratch::default(),
        }
    }
}

impl PartialEq for CompiledFieldCache {
    fn eq(&self, other: &Self) -> bool {
        self.latest_step == other.latest_step
            && self.latent_revision == other.latent_revision
            && self.query_bounds == other.query_bounds
            && self.field == other.field
    }
}

#[derive(Debug, Default)]
pub(crate) struct PlannerDecodeScratch {
    // CONTEXT: planner decode runs on every frame while the trail is active, so keep the
    // per-frame candidate and centerline working buffers in state rather than reallocating them.
    pub(crate) shade_profiles: Vec<ShadeProfile>,
    pub(crate) cell_candidates: BTreeMap<(i64, i64), Vec<CellCandidate>>,
    pub(crate) reusable_candidate_lists: Vec<Vec<CellCandidate>>,
    pub(crate) centerline_points: Vec<Point>,
    pub(crate) centerline_cumulative: Vec<f64>,
    pub(crate) centerline: Vec<CenterSample>,
    pub(crate) solver: SolverScratch,
}

#[derive(Debug, Default)]
pub(crate) struct SolverScratch {
    pub(crate) fallback_coords: Vec<(i64, i64)>,
    pub(crate) fallback_coord_index: HashMap<(i64, i64), usize>,
    pub(crate) fallback_assignment: Vec<Option<DecodedCellState>>,
    pub(crate) vote_buckets: HashMap<(i64, i64), Vec<StateVote>>,
    pub(crate) reusable_vote_lists: Vec<Vec<StateVote>>,
    pub(crate) compiled_row_index: BorrowedCellRowsScratch<CompiledCell>,
    pub(crate) candidate_row_index: BorrowedCellRowsScratch<Vec<CellCandidate>>,
}

pub(crate) struct PlanBuilder {
    pub(crate) viewport: Viewport,
    pub(crate) cell_ops: Vec<CellOp>,
    pub(crate) particle_ops: Vec<ParticleOp>,
    pub(crate) punch_through_cell: Option<(i64, i64)>,
}

impl PlanBuilder {
    pub(crate) fn with_capacity(
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

    pub(crate) fn in_bounds(&self, row: i64, col: i64) -> bool {
        row >= 1 && row <= self.viewport.max_row && col >= 1 && col <= self.viewport.max_col
    }

    pub(crate) fn set_punch_through_cell(&mut self, row: i64, col: i64) {
        self.punch_through_cell = Some((row, col));
    }

    pub(crate) fn is_punch_through_cell(&self, row: i64, col: i64) -> bool {
        self.punch_through_cell
            .is_some_and(|(target_row, target_col)| row == target_row && col == target_col)
    }

    pub(crate) fn push_cell(
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

    pub(crate) fn push_particle(
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

    pub(crate) fn finish(
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

pub(crate) struct PlanResources<'a> {
    pub(crate) builder: &'a mut PlanBuilder,
    pub(crate) windows_zindex: u32,
    pub(crate) particle_zindex: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CenterPathSample {
    pub(crate) step_index: StepIndex,
    pub(crate) pos: Point,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CenterSample {
    pub(crate) pos: Point,
    pub(crate) tangent_row: f64,
    pub(crate) tangent_col: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct SliceCell {
    pub(crate) coord: (i64, i64),
    pub(crate) normal_center_q16: i32,
    pub(crate) normal_span_q16: ProjectedSpanQ16,
    pub(crate) empty_cost: u64,
    pub(crate) non_empty_candidates: Vec<CellCandidate>,
}

impl SliceCell {
    pub(crate) fn new(
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
pub(crate) struct RibbonSlice {
    pub(crate) cells: Vec<SliceCell>,
    pub(crate) tail_u: f64,
    pub(crate) target_width_cells: f64,
    pub(crate) tip_width_cap_cells: f64,
    pub(crate) transverse_width_penalty: f64,
}

impl RibbonSlice {
    pub(crate) fn run_projected_span_q16(&self, state: SliceState) -> Option<ProjectedSpanQ16> {
        let run = state.run?;
        Some(
            self.cells[run.start]
                .normal_span_q16
                .cover(self.cells[run.end].normal_span_q16),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct SliceState {
    // NOTE: Keep run bounds as one optional value so partial ranges are unrepresentable.
    pub(crate) run: Option<RunSpan>,
    pub(crate) candidate_offsets: [u8; RIBBON_MAX_RUN_LENGTH],
    pub(crate) local_cost: u64,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct RunSpan {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl RunSpan {
    pub(crate) fn try_new(start: usize, end: usize) -> Option<Self> {
        (start <= end).then_some(Self { start, end })
    }

    pub(crate) fn contains(self, cell_index: usize) -> bool {
        cell_index >= self.start && cell_index <= self.end
    }
}

impl SliceState {
    pub(crate) fn empty(local_cost: u64) -> Self {
        Self {
            run: None,
            candidate_offsets: [0; RIBBON_MAX_RUN_LENGTH],
            local_cost,
        }
    }

    pub(crate) fn with_run(
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

    pub(crate) fn candidate_offset_for(self, cell_index: usize) -> Option<usize> {
        let run = self.run?;
        if !run.contains(cell_index) {
            return None;
        }
        Some(usize::from(self.candidate_offsets[cell_index - run.start]))
    }

    pub(crate) fn tie_break_key(self) -> (Option<RunSpan>, [u8; RIBBON_MAX_RUN_LENGTH]) {
        (self.run, self.candidate_offsets)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ProjectedSpanQ16 {
    pub(crate) min_q16: i32,
    pub(crate) max_q16: i32,
}

impl ProjectedSpanQ16 {
    pub(crate) fn try_new(min_q16: i32, max_q16: i32) -> Option<Self> {
        (min_q16 <= max_q16).then_some(Self { min_q16, max_q16 })
    }

    pub(crate) fn from_center_and_half_span(center_q16: i32, half_span_q16: i32) -> Self {
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

    pub(crate) fn width_cells(self) -> f64 {
        f64::from(self.max_q16.abs_diff(self.min_q16)) / Q16_SCALE as f64
    }

    pub(crate) fn center_q16(self) -> i32 {
        ((i64::from(self.min_q16) + i64::from(self.max_q16)) / 2) as i32
    }

    pub(crate) fn cover(self, other: Self) -> Self {
        Self {
            min_q16: self.min_q16.min(other.min_q16),
            max_q16: self.max_q16.max(other.max_q16),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct VoteStats {
    pub(crate) count: u32,
    pub(crate) total_cost: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct StateVote {
    pub(crate) state: Option<DecodedCellState>,
    pub(crate) stats: VoteStats,
}

pub(crate) const RIBBON_SAMPLE_SPACING_CELLS: f64 = 0.5;
pub(crate) const RIBBON_SLICE_HALF_SPAN: f64 = 0.60;
pub(crate) const RIBBON_MAX_CROSS_SECTION_CELLS: usize = 12;
pub(crate) const RIBBON_MAX_RUN_LENGTH: usize = 4;
pub(crate) const RIBBON_MAX_STATES_PER_SLICE: usize = 16;
pub(crate) const PREVIOUS_CELL_HALO_CELLS: i64 = 2;
pub(crate) const LOCAL_QUERY_ENVELOPE_FAST_PATH_MAX_AREA_CELLS: u64 = 4_096;
pub(crate) const FALLBACK_ITERATIONS: usize = 6;
pub(crate) const FALLBACK_COMPONENT_THRESHOLD: usize = 96;
pub(crate) const PENALTY_OVERLAP_FLIP: u64 = 16_000;
pub(crate) const PENALTY_OVERLAP_SHAPE: u64 = 6_000;
pub(crate) const PENALTY_EMPTY_TRANSITION: u64 = 5_500;
pub(crate) const PENALTY_THICKNESS_DELTA: u64 = 1_400;
pub(crate) const PENALTY_CENTER_SHIFT: u64 = 1_700;
pub(crate) const PENALTY_DISCONNECT: u64 = 18_000;
pub(crate) const Q16_SHIFT: u32 = 16;
pub(crate) const Q16_SCALE: i32 = 1 << Q16_SHIFT;
pub(crate) const Q16_SCALE_U64: u64 = 1_u64 << Q16_SHIFT;
pub(crate) const WIDTH_Q10_SCALE_U64: u64 = 1_024;
pub(crate) const SPEED_SHEATH_START_CPS: f64 = 7.0;
pub(crate) const SPEED_SHEATH_FULL_CPS: f64 = 28.0;
pub(crate) const SPEED_SHEATH_MIN_GAIN: f64 = 0.08;
pub(crate) const SPEED_CORE_START_CPS: f64 = 10.0;
pub(crate) const SPEED_CORE_FULL_CPS: f64 = 34.0;
pub(crate) const SPEED_CORE_MIN_GAIN: f64 = 0.78;
pub(crate) const COMET_NECK_FRACTION: f64 = 0.14;
pub(crate) const COMET_TIP_WIDTH_RATIO: f64 = 0.26;
pub(crate) const COMET_TAPER_EXPONENT: f64 = 1.90;
pub(crate) const COMET_MIN_RESOLVABLE_WIDTH: f64 = 0.26;
pub(crate) const COMET_TIP_ZONE_START: f64 = 0.80;
pub(crate) const COMET_TIP_CAP_MULTIPLIER: f64 = 1.00;
pub(crate) const COMET_MONO_EPSILON_CELLS: f64 = 0.10;
pub(crate) const COMET_CURVATURE_COMPRESS_FACTOR: f64 = 1.05;
pub(crate) const COMET_CURVATURE_KAPPA_CAP: f64 = 1.25;
pub(crate) const COMET_TAPER_WEIGHT: u64 = 3_400;
pub(crate) const COMET_MONO_WEIGHT: u64 = 7_800;
pub(crate) const COMET_TIP_WEIGHT: u64 = 11_800;
pub(crate) const COMET_TRANSVERSE_WEIGHT: u64 = 900;
pub(crate) const CATCH_SALIENCE_DIM_PENALTY: u64 = 100;

pub(crate) fn saturating_q16_offset(base: i32, delta: i32) -> i32 {
    (i64::from(base) + i64::from(delta)).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

pub(crate) fn hash_f64(hasher: &mut DefaultHasher, value: f64) {
    value.to_bits().hash(hasher);
}

#[cfg(test)]
pub(crate) fn pose_for_frame(frame: &RenderFrame) -> super::super::latent_field::Pose {
    pose_for_corners(&frame.corners, &frame.target_corners)
}

pub(crate) fn pose_for_corners(
    corners: &[Point; 4],
    target_corners: &[Point; 4],
) -> super::super::latent_field::Pose {
    let center = crate::types::corners_center(corners);
    let width = (target_corners[1].col - target_corners[0].col)
        .abs()
        .max(1.0 / 8.0);
    let height = (target_corners[3].row - target_corners[0].row)
        .abs()
        .max(1.0 / 8.0);

    super::super::latent_field::Pose {
        center,
        half_height: height * 0.5,
        half_width: width * 0.5,
    }
}

pub(crate) fn pose_for_step_sample(
    sample: &RenderStepSample,
    frame: &RenderFrame,
) -> super::super::latent_field::Pose {
    pose_for_corners(&sample.corners, &frame.target_corners)
}

pub(crate) fn frame_sample_poses(
    frame: &RenderFrame,
) -> impl Iterator<Item = super::super::latent_field::Pose> + '_ {
    frame
        .step_samples
        .iter()
        .map(|sample| pose_for_step_sample(sample, frame))
}

//! Shell-facing realization helpers for scene patches and render plans.
//!
//! This layer normalizes logical planner output into the draw, clear, and noop
//! payloads that the host bridge can apply, keeping missing-basis failures as
//! explicit lifecycle results instead of hidden exceptions.

use crate::core::state::BackgroundProbeBatch;
#[cfg(test)]
use crate::core::state::ProjectionSnapshot;
#[cfg(test)]
use crate::core::state::ScenePatch;
#[cfg(test)]
use crate::core::state::ScenePatchKind;
use crate::draw::PARTICLE_ZINDEX_OFFSET;
use crate::draw::render_plan::CellOp;
use crate::draw::render_plan::ClearOp;
use crate::draw::render_plan::Glyph;
use crate::draw::render_plan::HighlightRef;
use crate::draw::render_plan::RenderPlan;
use crate::draw::render_plan::TargetCellOverlay;
use crate::draw::render_plan::Viewport;
use crate::octant_chars::OCTANT_CHARACTERS;
use crate::types::ModeClass;
use crate::types::PlannerFrame;
use crate::types::RenderFrame;
use crate::types::ScreenCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::sync::Arc;
#[cfg(test)]
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PaletteSpec {
    mode: ModeClass,
    cursor_color: Option<String>,
    cursor_color_insert_mode: Option<String>,
    normal_bg: Option<String>,
    transparent_bg_fallback_color: String,
    cterm_cursor_colors: Option<Vec<u16>>,
    cterm_bg: Option<u16>,
    color_levels: u32,
    gamma_bits: u64,
    color_at_cursor: Option<u32>,
}

impl PaletteSpec {
    pub(crate) fn from_frame(frame: &RenderFrame) -> Self {
        let static_config = frame.static_config.as_ref();
        Self {
            mode: frame.mode,
            cursor_color: static_config.cursor_color.clone(),
            cursor_color_insert_mode: static_config.cursor_color_insert_mode.clone(),
            normal_bg: static_config.normal_bg.clone(),
            transparent_bg_fallback_color: static_config.transparent_bg_fallback_color.clone(),
            cterm_cursor_colors: static_config.cterm_cursor_colors.clone(),
            cterm_bg: static_config.cterm_bg,
            color_levels: static_config.color_levels,
            gamma_bits: static_config.gamma.to_bits(),
            color_at_cursor: frame.color_at_cursor,
        }
    }

    pub(crate) const fn mode(&self) -> ModeClass {
        self.mode
    }

    pub(crate) fn cursor_color(&self) -> Option<&str> {
        self.cursor_color.as_deref()
    }

    pub(crate) fn cursor_color_insert_mode(&self) -> Option<&str> {
        self.cursor_color_insert_mode.as_deref()
    }

    pub(crate) fn normal_bg(&self) -> Option<&str> {
        self.normal_bg.as_deref()
    }

    pub(crate) fn transparent_bg_fallback_color(&self) -> &str {
        &self.transparent_bg_fallback_color
    }

    pub(crate) fn cterm_cursor_colors(&self) -> Option<&[u16]> {
        self.cterm_cursor_colors.as_deref()
    }

    pub(crate) fn cterm_bg(&self) -> Option<u16> {
        self.cterm_bg
    }

    pub(crate) fn color_levels(&self) -> u32 {
        self.color_levels.max(1)
    }

    pub(crate) fn gamma(&self) -> f64 {
        f64::from_bits(self.gamma_bits)
    }

    pub(crate) fn gamma_bits(&self) -> u64 {
        self.gamma_bits
    }

    pub(crate) const fn color_at_cursor(&self) -> Option<u32> {
        self.color_at_cursor
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct LogicalRaster {
    clear: Option<ClearOp>,
    particle_cells: Arc<[CellOp]>,
    static_cells: Arc<[CellOp]>,
}

impl LogicalRaster {
    #[cfg(test)]
    pub(crate) fn new(clear: Option<ClearOp>, cells: Arc<[CellOp]>) -> Self {
        Self {
            clear,
            particle_cells: Arc::default(),
            static_cells: cells,
        }
    }

    pub(crate) fn from_segments(
        clear: Option<ClearOp>,
        particle_cells: Arc<[CellOp]>,
        static_cells: Arc<[CellOp]>,
    ) -> Self {
        Self {
            clear,
            particle_cells,
            static_cells,
        }
    }

    pub(crate) const fn clear(&self) -> Option<ClearOp> {
        self.clear
    }

    pub(crate) fn particle_cells(&self) -> &[CellOp] {
        self.particle_cells.as_ref()
    }

    pub(crate) fn static_cells(&self) -> &[CellOp] {
        self.static_cells.as_ref()
    }

    pub(crate) fn iter_cells(&self) -> impl Iterator<Item = &CellOp> {
        self.particle_cells.iter().chain(self.static_cells.iter())
    }

    #[cfg(test)]
    pub(crate) fn collected_cells(&self) -> Vec<CellOp> {
        self.iter_cells().copied().collect()
    }

    pub(crate) fn into_particle_cells(self) -> Arc<[CellOp]> {
        self.particle_cells
    }

    pub(crate) fn replace_particle_cells(&self, particle_cells: Arc<[CellOp]>) -> Self {
        Self::from_segments(self.clear, particle_cells, Arc::clone(&self.static_cells))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct RealizationSpanChunk {
    glyph: Glyph,
    highlight: HighlightRef,
}

impl RealizationSpanChunk {
    pub(crate) const fn new(glyph: Glyph, highlight: HighlightRef) -> Self {
        Self { glyph, highlight }
    }

    pub(crate) const fn glyph(&self) -> Glyph {
        self.glyph
    }

    pub(crate) const fn highlight(&self) -> HighlightRef {
        self.highlight
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct RealizationSpan {
    row: i64,
    col: i64,
    width: u32,
    zindex: u32,
    payload_hash: u64,
    chunks: Arc<[RealizationSpanChunk]>,
}

impl RealizationSpan {
    fn payload_hash_for_chunks(chunks: &[RealizationSpanChunk]) -> u64 {
        let mut hasher = DefaultHasher::new();
        for chunk in chunks {
            chunk.glyph().hash(&mut hasher);
            chunk.highlight().hash(&mut hasher);
        }
        hasher.finish()
    }

    pub(crate) const fn row(&self) -> i64 {
        self.row
    }

    pub(crate) const fn col(&self) -> i64 {
        self.col
    }

    pub(crate) const fn width(&self) -> u32 {
        self.width
    }

    pub(crate) const fn zindex(&self) -> u32 {
        self.zindex
    }

    pub(crate) const fn payload_hash(&self) -> u64 {
        self.payload_hash
    }

    pub(crate) fn chunks(&self) -> &[RealizationSpanChunk] {
        self.chunks.as_ref()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct RealizationProjection {
    clear: Option<ClearOp>,
    particle_spans: Arc<[RealizationSpan]>,
    static_spans: Arc<[RealizationSpan]>,
}

impl RealizationProjection {
    pub(crate) fn from_segments(
        clear: Option<ClearOp>,
        particle_spans: Arc<[RealizationSpan]>,
        static_spans: Arc<[RealizationSpan]>,
    ) -> Self {
        Self {
            clear,
            particle_spans,
            static_spans,
        }
    }

    pub(crate) const fn clear(&self) -> Option<ClearOp> {
        self.clear
    }

    pub(crate) fn particle_spans(&self) -> &[RealizationSpan] {
        self.particle_spans.as_ref()
    }

    pub(crate) fn static_spans(&self) -> &[RealizationSpan] {
        self.static_spans.as_ref()
    }

    pub(crate) fn spans(&self) -> impl Iterator<Item = &RealizationSpan> + Clone {
        self.particle_spans.iter().chain(self.static_spans.iter())
    }

    pub(crate) fn span_count(&self) -> usize {
        self.particle_spans
            .len()
            .saturating_add(self.static_spans.len())
    }

    pub(crate) fn replace_particle_spans(&self, particle_spans: Arc<[RealizationSpan]>) -> Self {
        Self::from_segments(self.clear, particle_spans, Arc::clone(&self.static_spans))
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ScenePatchRealization<'a> {
    Draw(&'a ProjectionSnapshot),
    Clear,
    Noop,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub(crate) enum ScenePatchRealizationError {
    #[error("replace patch missing target projection")]
    MissingTargetProjection,
}

/// Resolves one scene patch into the shell-facing draw, clear, or noop input.
#[cfg(test)]
pub(crate) fn project_scene_patch(
    patch: &ScenePatch,
) -> Result<ScenePatchRealization<'_>, ScenePatchRealizationError> {
    match patch.kind() {
        ScenePatchKind::Noop => Ok(ScenePatchRealization::Noop),
        ScenePatchKind::Clear => Ok(ScenePatchRealization::Clear),
        ScenePatchKind::Replace => patch
            .basis()
            .target()
            // Surprising: a replace patch without a target projection breaks the
            // phase-4 basis invariant and leaves shell apply without draw input.
            .map(ScenePatchRealization::Draw)
            .ok_or(ScenePatchRealizationError::MissingTargetProjection),
    }
}

#[derive(Debug, Default)]
struct RealizationSpanBuilder {
    spans: Vec<RealizationSpan>,
    pending: Option<PendingSpan>,
}

#[derive(Debug)]
struct PendingSpan {
    row: i64,
    col: i64,
    width: u32,
    zindex: u32,
    chunks: Vec<RealizationSpanChunk>,
    payload_hasher: DefaultHasher,
}

impl RealizationSpanBuilder {
    fn flush_pending(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };

        self.spans.push(RealizationSpan {
            row: pending.row,
            col: pending.col,
            width: pending.width,
            zindex: pending.zindex,
            payload_hash: pending.payload_hasher.finish(),
            chunks: Arc::from(pending.chunks),
        });
    }

    fn push_cell(&mut self, cell: &CellOp) {
        let can_append = self.pending.as_ref().is_some_and(|pending| {
            pending.row == cell.row
                && pending.zindex == cell.zindex
                && cell.col == pending.col.saturating_add(i64::from(pending.width))
        });
        if !can_append {
            self.flush_pending();
            self.pending = Some(PendingSpan {
                row: cell.row,
                col: cell.col,
                width: 0,
                zindex: cell.zindex,
                chunks: Vec::new(),
                payload_hasher: DefaultHasher::new(),
            });
        }

        let Some(pending) = self.pending.as_mut() else {
            return;
        };
        pending.width = pending.width.saturating_add(1);
        cell.glyph.hash(&mut pending.payload_hasher);
        cell.highlight.hash(&mut pending.payload_hasher);
        pending
            .chunks
            .push(RealizationSpanChunk::new(cell.glyph, cell.highlight));
    }

    fn finish(mut self) -> Arc<[RealizationSpan]> {
        self.flush_pending();
        Arc::from(self.spans)
    }
}

fn in_bounds(viewport: Viewport, row: i64, col: i64) -> bool {
    row >= 1 && row <= viewport.max_row && col >= 1 && col <= viewport.max_col
}

fn overlay_cell(overlay: TargetCellOverlay) -> CellOp {
    CellOp {
        row: overlay.row,
        col: overlay.col,
        zindex: overlay.zindex,
        glyph: Glyph::Static(overlay.shape.glyph()),
        highlight: HighlightRef::Normal(overlay.level),
    }
}

pub(crate) fn project_cell_ops_to_spans<'a>(
    ops: impl IntoIterator<Item = &'a CellOp>,
) -> Arc<[RealizationSpan]> {
    let mut builder = RealizationSpanBuilder::default();
    for op in ops {
        builder.push_cell(op);
    }
    builder.finish()
}

pub(crate) fn realize_particle_cells(particle_cells: &[CellOp]) -> Arc<[RealizationSpan]> {
    project_cell_ops_to_spans(particle_cells.iter())
}

pub(crate) fn realize_logical_raster(raster: &LogicalRaster) -> RealizationProjection {
    RealizationProjection::from_segments(
        raster.clear(),
        realize_particle_cells(raster.particle_cells()),
        project_cell_ops_to_spans(raster.static_cells().iter()),
    )
}

fn push_projected_particle_cell(
    cells: &mut Vec<CellOp>,
    cell: CellOp,
    requires_background_probe: bool,
    viewport: Viewport,
    background_probe: Option<&BackgroundProbeBatch>,
) {
    if !in_bounds(viewport, cell.row, cell.col) {
        return;
    }
    if requires_background_probe {
        let Some(screen_cell) = ScreenCell::new(cell.row, cell.col) else {
            return;
        };
        if !background_probe.is_some_and(|probe| probe.allows_particle(screen_cell)) {
            return;
        }
    }

    cells.push(cell);
}

fn project_particle_ops(
    particle_ops: &[crate::draw::render_plan::ParticleOp],
    viewport: Viewport,
    background_probe: Option<&BackgroundProbeBatch>,
) -> Arc<[CellOp]> {
    let mut cells = Vec::<CellOp>::with_capacity(particle_ops.len());

    for op in particle_ops {
        push_projected_particle_cell(
            &mut cells,
            op.cell,
            op.requires_background_probe,
            viewport,
            background_probe,
        );
    }

    Arc::from(cells)
}

pub(crate) fn project_particle_overlay_cells(
    frame: &PlannerFrame,
    viewport: Viewport,
    background_probe: Option<&BackgroundProbeBatch>,
) -> Arc<[CellOp]> {
    if !frame.has_particles() {
        return Arc::default();
    }

    let target_row = frame.target.row.round() as i64;
    let target_col = frame.target.col.round() as i64;
    let particle_zindex = frame.windows_zindex.saturating_sub(PARTICLE_ZINDEX_OFFSET);
    let particle_max_lifetime = if frame.particle_max_lifetime.is_finite() {
        frame.particle_max_lifetime.max(0.0)
    } else {
        0.0
    };
    let switch_ratio = if frame.particle_switch_octant_braille.is_finite() {
        frame.particle_switch_octant_braille.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let lifetime_switch_octant_braille = switch_ratio * particle_max_lifetime;
    let requires_background_probe = !frame.particles_over_text;
    let mut cells = Vec::<CellOp>::with_capacity(frame.aggregated_particle_cells().len());

    // Keep overlay refresh on the cheap path: rebuild only the particle cells directly from the
    // retained aggregate instead of materializing a temporary one-off RenderPlan.
    for aggregate in frame.aggregated_particle_cells() {
        let row = aggregate.row();
        let col = aggregate.col();
        if row == target_row && col == target_col {
            continue;
        }

        let Some(lifetime_average) = aggregate.lifetime_average() else {
            continue;
        };

        let shade = if lifetime_average > lifetime_switch_octant_braille {
            let denominator = (particle_max_lifetime - lifetime_switch_octant_braille).max(1.0e-9);
            ((lifetime_average - lifetime_switch_octant_braille) / denominator).clamp(0.0, 1.0)
        } else {
            let denominator = lifetime_switch_octant_braille.max(1.0e-9);
            (lifetime_average / denominator).clamp(0.0, 1.0)
        };
        if !shade.is_finite() || frame.color_levels == 0 {
            continue;
        }

        let rounded_level = ((shade * f64::from(frame.color_levels)) + 0.5).floor() as i64;
        if rounded_level <= 0 {
            continue;
        }
        let clamped_level = rounded_level.min(i64::from(frame.color_levels));
        let Some(level) = u32::try_from(clamped_level)
            .ok()
            .and_then(crate::draw::render_plan::HighlightLevel::try_new)
        else {
            continue;
        };

        let cell = aggregate.cell();
        let glyph = if lifetime_average > lifetime_switch_octant_braille {
            let octant_index = usize::from(cell[0][0] > 0.0)
                + usize::from(cell[0][1] > 0.0) * 2
                + usize::from(cell[1][0] > 0.0) * 4
                + usize::from(cell[1][1] > 0.0) * 8
                + usize::from(cell[2][0] > 0.0) * 16
                + usize::from(cell[2][1] > 0.0) * 32
                + usize::from(cell[3][0] > 0.0) * 64
                + usize::from(cell[3][1] > 0.0) * 128;
            if octant_index == 0 {
                continue;
            }
            let Some(character) = OCTANT_CHARACTERS
                .get(octant_index.saturating_sub(1))
                .copied()
            else {
                continue;
            };
            Glyph::Static(character)
        } else {
            let braille_index = usize::from(cell[0][0] > 0.0)
                + usize::from(cell[1][0] > 0.0) * 2
                + usize::from(cell[2][0] > 0.0) * 4
                + usize::from(cell[0][1] > 0.0) * 8
                + usize::from(cell[1][1] > 0.0) * 16
                + usize::from(cell[2][1] > 0.0) * 32
                + usize::from(cell[3][0] > 0.0) * 64
                + usize::from(cell[3][1] > 0.0) * 128;
            if braille_index == 0 {
                continue;
            }
            let Ok(character) = u8::try_from(braille_index) else {
                continue;
            };
            Glyph::Braille(character)
        };

        push_projected_particle_cell(
            &mut cells,
            CellOp {
                row,
                col,
                zindex: particle_zindex,
                glyph,
                highlight: HighlightRef::Normal(level),
            },
            requires_background_probe,
            viewport,
            background_probe,
        );
    }

    Arc::from(cells)
}

pub(crate) fn project_render_plan(
    plan: &RenderPlan,
    viewport: Viewport,
    background_probe: Option<&BackgroundProbeBatch>,
) -> LogicalRaster {
    let particle_cells = project_particle_ops(&plan.particle_ops, viewport, background_probe);
    let mut static_cells = Vec::<CellOp>::with_capacity(plan.cell_ops.len() + 1);

    static_cells.extend(
        plan.cell_ops
            .iter()
            .filter(|op| in_bounds(viewport, op.row, op.col))
            .copied(),
    );

    if let Some(overlay) = plan.target_cell_overlay
        && in_bounds(viewport, overlay.row, overlay.col)
    {
        static_cells.push(overlay_cell(overlay));
    }

    // background-dependent particle admission is resolved from the explicit
    // observation probe before snapshot retention. The retained projection keeps the
    // logical raster and caches the realized spans for shell apply reuse.
    LogicalRaster::from_segments(plan.clear, particle_cells, Arc::from(static_cells))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::PatchBasis;
    use crate::core::state::ProjectionSnapshot;
    use crate::core::state::ProjectionWitness;
    use crate::core::state::ScenePatch;
    use crate::core::types::CursorCol;
    use crate::core::types::CursorRow;
    use crate::core::types::IngressSeq;
    use crate::core::types::ObservationId;
    use crate::core::types::ProjectorRevision;
    use crate::core::types::SceneRevision;
    use crate::core::types::StrokeId;
    use crate::core::types::ViewportSnapshot;
    use crate::draw::render_plan::HighlightLevel;
    use crate::draw::render_plan::ParticleOp;
    use crate::draw::render_plan::PlannerState;
    use crate::draw::render_plan::render_frame_to_plan;
    use crate::test_support::proptest::pure_config;
    use crate::types::BASE_TIME_INTERVAL;
    use crate::types::CursorCellShape;
    use crate::types::ModeClass;
    use crate::types::Particle;
    use crate::types::PlannerFrame;
    use crate::types::Point;
    use crate::types::RenderFrame;
    use crate::types::RenderStepSample;
    use crate::types::StaticRenderConfig;
    use pretty_assertions::assert_eq;
    use proptest::collection::vec;
    use proptest::option;
    use proptest::prelude::*;
    use std::sync::Arc;

    fn sample_for_corners(corners: [Point; 4]) -> RenderStepSample {
        RenderStepSample::new(corners, BASE_TIME_INTERVAL)
    }

    fn representative_frame() -> RenderFrame {
        let corners = [
            Point {
                row: 10.0,
                col: 10.0,
            },
            Point {
                row: 10.0,
                col: 11.0,
            },
            Point {
                row: 11.0,
                col: 11.0,
            },
            Point {
                row: 11.0,
                col: 10.0,
            },
        ];
        RenderFrame {
            mode: ModeClass::NormalLike,
            corners,
            step_samples: vec![sample_for_corners(corners)].into(),
            planner_idle_steps: 0,
            target: Point {
                row: 10.0,
                col: 10.0,
            },
            target_corners: corners,
            vertical_bar: false,
            trail_stroke_id: StrokeId::new(1),
            retarget_epoch: 1,
            particle_count: 0,
            aggregated_particle_cells: Arc::default(),
            particle_screen_cells: Arc::default(),
            color_at_cursor: None,
            static_config: Arc::new(StaticRenderConfig {
                cursor_color: None,
                cursor_color_insert_mode: None,
                normal_bg: None,
                transparent_bg_fallback_color: "#303030".to_string(),
                cterm_cursor_colors: None,
                cterm_bg: None,
                hide_target_hack: false,
                max_kept_windows: 32,
                never_draw_over_target: false,
                particle_max_lifetime: 1.0,
                particle_switch_octant_braille: 0.3,
                particles_over_text: true,
                color_levels: 16,
                gamma: 2.2,
                block_aspect_ratio: crate::config::DEFAULT_BLOCK_ASPECT_RATIO,
                tail_duration_ms: 180.0,
                simulation_hz: 120.0,
                trail_thickness: 1.0,
                trail_thickness_x: 1.0,
                spatial_coherence_weight: 1.0,
                temporal_stability_weight: 0.12,
                top_k_per_cell: 5,
                windows_zindex: 200,
            }),
        }
    }

    fn flatten_projection_cells(projection: &RealizationProjection) -> Vec<CellOp> {
        let mut cells = Vec::new();
        for span in projection.spans() {
            for (offset, chunk) in span.chunks().iter().enumerate() {
                cells.push(CellOp {
                    row: span.row(),
                    col: span
                        .col()
                        .saturating_add(i64::try_from(offset).unwrap_or(i64::MAX)),
                    zindex: span.zindex(),
                    glyph: chunk.glyph(),
                    highlight: chunk.highlight(),
                });
            }
        }
        cells
    }

    fn legacy_visible_cells_without_probe(plan: &RenderPlan, viewport: Viewport) -> Vec<CellOp> {
        let mut cells = plan
            .particle_ops
            .iter()
            .filter(|op| {
                !op.requires_background_probe && in_bounds(viewport, op.cell.row, op.cell.col)
            })
            .map(|op| op.cell)
            .collect::<Vec<_>>();
        cells.extend(
            plan.cell_ops
                .iter()
                .filter(|op| in_bounds(viewport, op.row, op.col))
                .copied(),
        );
        if let Some(overlay) = plan.target_cell_overlay
            && in_bounds(viewport, overlay.row, overlay.col)
        {
            cells.push(overlay_cell(overlay));
        }
        cells
    }

    fn glyph_strategy() -> BoxedStrategy<Glyph> {
        prop_oneof![Just(Glyph::BLOCK), (1_u8..=4_u8).prop_map(Glyph::Braille),].boxed()
    }

    fn highlight_ref_strategy() -> BoxedStrategy<HighlightRef> {
        (1_u32..=6_u32)
            .prop_map(|level| HighlightRef::Normal(HighlightLevel::from_raw_clamped(level)))
            .boxed()
    }

    fn cursor_cell_shape() -> BoxedStrategy<CursorCellShape> {
        prop_oneof![
            Just(CursorCellShape::Block),
            Just(CursorCellShape::VerticalBar),
            Just(CursorCellShape::HorizontalBar),
        ]
        .boxed()
    }

    fn cell_op_strategy(max_row: u32, max_col: u32) -> BoxedStrategy<CellOp> {
        let row_limit = i64::from(max_row).saturating_add(2);
        let col_limit = i64::from(max_col).saturating_add(2);

        (
            -1_i64..=row_limit,
            -1_i64..=col_limit,
            1_u32..=4_u32,
            glyph_strategy(),
            highlight_ref_strategy(),
        )
            .prop_map(|(row, col, zindex, glyph, highlight)| CellOp {
                row,
                col,
                zindex,
                glyph,
                highlight,
            })
            .boxed()
    }

    fn particle_op_strategy(max_row: u32, max_col: u32) -> BoxedStrategy<ParticleOp> {
        (cell_op_strategy(max_row, max_col), any::<bool>())
            .prop_map(|(cell, requires_background_probe)| ParticleOp {
                cell,
                requires_background_probe,
            })
            .boxed()
    }

    fn target_cell_overlay_strategy(
        max_row: u32,
        max_col: u32,
    ) -> BoxedStrategy<TargetCellOverlay> {
        let row_limit = i64::from(max_row).saturating_add(2);
        let col_limit = i64::from(max_col).saturating_add(2);

        (
            -1_i64..=row_limit,
            -1_i64..=col_limit,
            1_u32..=4_u32,
            cursor_cell_shape(),
            1_u32..=6_u32,
        )
            .prop_map(|(row, col, zindex, shape, level)| TargetCellOverlay {
                row,
                col,
                zindex,
                shape,
                level: HighlightLevel::from_raw_clamped(level),
            })
            .boxed()
    }

    #[derive(Clone, Debug)]
    struct RenderPlanFixture {
        viewport: Viewport,
        viewport_snapshot: ViewportSnapshot,
        plan: RenderPlan,
        background_probe: Option<BackgroundProbeBatch>,
    }

    fn render_plan_fixture() -> BoxedStrategy<RenderPlanFixture> {
        (1_u32..=6_u32, 1_u32..=6_u32)
            .prop_flat_map(|(max_row, max_col)| {
                let viewport = Viewport {
                    max_row: i64::from(max_row),
                    max_col: i64::from(max_col),
                };
                let viewport_snapshot =
                    ViewportSnapshot::new(CursorRow(max_row), CursorCol(max_col));
                let mask_len = usize::try_from(max_row.saturating_mul(max_col)).unwrap_or(0);

                (
                    option::of(
                        (0_usize..=32_usize)
                            .prop_map(|max_kept_windows| ClearOp { max_kept_windows }),
                    ),
                    vec(cell_op_strategy(max_row, max_col), 0..=6),
                    vec(particle_op_strategy(max_row, max_col), 0..=6),
                    option::of(target_cell_overlay_strategy(max_row, max_col)),
                    option::of(vec(any::<bool>(), mask_len)),
                )
                    .prop_map(
                        move |(
                            clear,
                            cell_ops,
                            particle_ops,
                            target_cell_overlay,
                            allowed_mask,
                        )| {
                            let background_probe = allowed_mask.map(|allowed_mask| {
                                BackgroundProbeBatch::from_allowed_mask(
                                    viewport_snapshot,
                                    allowed_mask,
                                )
                            });
                            RenderPlanFixture {
                                viewport,
                                viewport_snapshot,
                                plan: RenderPlan {
                                    clear,
                                    cell_ops,
                                    particle_ops,
                                    target_cell_overlay,
                                },
                                background_probe,
                            }
                        },
                    )
            })
            .boxed()
    }

    fn expected_visible_cells(
        plan: &RenderPlan,
        viewport: Viewport,
        background_probe: Option<&BackgroundProbeBatch>,
    ) -> Vec<CellOp> {
        let mut cells = Vec::new();

        for op in &plan.particle_ops {
            if !in_bounds(viewport, op.cell.row, op.cell.col) {
                continue;
            }
            if op.requires_background_probe {
                let Some(screen_cell) = ScreenCell::new(op.cell.row, op.cell.col) else {
                    continue;
                };
                if background_probe.is_some_and(|probe| probe.allows_particle(screen_cell)) {
                    cells.push(op.cell);
                }
            } else {
                cells.push(op.cell);
            }
        }

        cells.extend(
            plan.cell_ops
                .iter()
                .filter(|op| in_bounds(viewport, op.row, op.col))
                .copied(),
        );

        if let Some(overlay) = plan.target_cell_overlay
            && in_bounds(viewport, overlay.row, overlay.col)
        {
            cells.push(overlay_cell(overlay));
        }

        cells
    }

    #[derive(Clone, Debug)]
    struct ScenePatchFixture {
        acknowledged: Option<ProjectionSnapshot>,
        target: Option<ProjectionSnapshot>,
        expected_kind: ScenePatchKind,
    }

    fn witness(observation_seq: u64, max_row: u32, max_col: u32) -> ProjectionWitness {
        ProjectionWitness::new(
            SceneRevision::INITIAL,
            ObservationId::from_ingress_seq(IngressSeq::new(observation_seq)),
            ViewportSnapshot::new(CursorRow(max_row), CursorCol(max_col)),
            ProjectorRevision::CURRENT,
        )
    }

    fn snapshot_with(
        observation_seq: u64,
        max_row: u32,
        max_col: u32,
        max_kept_windows: usize,
    ) -> ProjectionSnapshot {
        ProjectionSnapshot::new(
            witness(observation_seq, max_row, max_col),
            LogicalRaster::new(
                Some(ClearOp { max_kept_windows }),
                Arc::from(Vec::<CellOp>::new()),
            ),
        )
    }

    fn scene_patch_fixture() -> BoxedStrategy<ScenePatchFixture> {
        prop_oneof![
            Just(ScenePatchFixture {
                acknowledged: None,
                target: None,
                expected_kind: ScenePatchKind::Noop,
            }),
            (1_u32..=20_u32).prop_map(|max_kept_windows| {
                let acknowledged =
                    snapshot_with(1, 20, 40, usize::try_from(max_kept_windows).unwrap_or(0));
                ScenePatchFixture {
                    acknowledged: Some(acknowledged),
                    target: None,
                    expected_kind: ScenePatchKind::Clear,
                }
            }),
            (1_u32..=20_u32).prop_map(|max_kept_windows| {
                let target =
                    snapshot_with(1, 20, 40, usize::try_from(max_kept_windows).unwrap_or(0));
                ScenePatchFixture {
                    acknowledged: None,
                    target: Some(target),
                    expected_kind: ScenePatchKind::Replace,
                }
            }),
            (1_u32..=20_u32).prop_map(|max_kept_windows| {
                let snapshot =
                    snapshot_with(1, 20, 40, usize::try_from(max_kept_windows).unwrap_or(0));
                ScenePatchFixture {
                    acknowledged: Some(snapshot.clone()),
                    target: Some(snapshot),
                    expected_kind: ScenePatchKind::Noop,
                }
            }),
            (1_u32..=20_u32).prop_map(|max_kept_windows| {
                let max_kept_windows = usize::try_from(max_kept_windows).unwrap_or(0);
                let acknowledged = snapshot_with(1, 20, 40, max_kept_windows);
                let target = acknowledged.clone().with_witness(witness(2, 20, 40));
                ScenePatchFixture {
                    acknowledged: Some(acknowledged),
                    target: Some(target),
                    expected_kind: ScenePatchKind::Replace,
                }
            }),
            (1_u32..=20_u32, any::<bool>()).prop_map(|(left, increase)| {
                let right = if increase {
                    left.saturating_add(1)
                } else if left == 1 {
                    2
                } else {
                    left.saturating_sub(1)
                };
                let acknowledged = snapshot_with(1, 20, 40, usize::try_from(left).unwrap_or(0));
                let target = snapshot_with(1, 20, 40, usize::try_from(right).unwrap_or(0));
                ScenePatchFixture {
                    acknowledged: Some(acknowledged),
                    target: Some(target),
                    expected_kind: ScenePatchKind::Replace,
                }
            }),
        ]
        .boxed()
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_project_render_plan_keeps_exactly_the_visible_and_background_allowed_cells(
            fixture in render_plan_fixture(),
        ) {
            let background_probe = fixture.background_probe.as_ref();
            let raster = project_render_plan(&fixture.plan, fixture.viewport, background_probe);
            let realized = realize_logical_raster(&raster);

            let expected =
                expected_visible_cells(&fixture.plan, fixture.viewport, background_probe);

            assert_eq!(raster.clear(), fixture.plan.clear);
            assert_eq!(raster.collected_cells(), expected);
            assert_eq!(flatten_projection_cells(&realized), expected);
            if let Some(background_probe) = fixture.background_probe.as_ref() {
                prop_assert_eq!(background_probe.viewport(), fixture.viewport_snapshot);
            }
        }

        #[test]
        fn prop_scene_patch_kind_and_realization_follow_only_the_basis_pair(
            fixture in scene_patch_fixture(),
        ) {
            let patch =
                ScenePatch::derive(PatchBasis::new(fixture.acknowledged.clone(), fixture.target.clone()));

            assert_eq!(patch.kind(), fixture.expected_kind);
            match (fixture.expected_kind, fixture.target.as_ref()) {
                (ScenePatchKind::Noop, _) => {
                    assert_eq!(project_scene_patch(&patch), Ok(ScenePatchRealization::Noop));
                }
                (ScenePatchKind::Clear, _) => {
                    assert_eq!(project_scene_patch(&patch), Ok(ScenePatchRealization::Clear));
                }
                (ScenePatchKind::Replace, Some(target)) => {
                    assert_eq!(project_scene_patch(&patch), Ok(ScenePatchRealization::Draw(target)));
                }
                (ScenePatchKind::Replace, None) => {
                    prop_assert_eq!(
                        project_scene_patch(&patch),
                        Err(ScenePatchRealizationError::MissingTargetProjection),
                    );
                }
            }
        }
    }

    #[test]
    fn realization_projection_preserves_legacy_render_plan_cells_for_representative_frame() {
        let frame = representative_frame();
        let viewport = Viewport {
            max_row: 40,
            max_col: 80,
        };
        let planner_output = render_frame_to_plan(
            &PlannerFrame::from_render_frame(&frame),
            PlannerState::default(),
            viewport,
        );
        let raster = project_render_plan(&planner_output.plan, viewport, None);
        let realized = realize_logical_raster(&raster);

        assert_eq!(
            flatten_projection_cells(&realized),
            legacy_visible_cells_without_probe(&planner_output.plan, viewport)
        );
    }

    #[test]
    fn replace_particle_cells_reuses_the_static_segment() {
        let original_particle_cells: Arc<[CellOp]> = Arc::from(vec![CellOp {
            row: 3,
            col: 4,
            zindex: 50,
            glyph: Glyph::Braille(1),
            highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(1)),
        }]);
        let static_cells: Arc<[CellOp]> = Arc::from(vec![
            CellOp {
                row: 4,
                col: 5,
                zindex: 60,
                glyph: Glyph::BLOCK,
                highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(2)),
            },
            CellOp {
                row: 4,
                col: 6,
                zindex: 60,
                glyph: Glyph::Static("x"),
                highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(3)),
            },
        ]);
        let raster = LogicalRaster::from_segments(
            Some(ClearOp {
                max_kept_windows: 4,
            }),
            Arc::clone(&original_particle_cells),
            Arc::clone(&static_cells),
        );
        let replacement_particle_cells: Arc<[CellOp]> = Arc::from(vec![CellOp {
            row: 8,
            col: 9,
            zindex: 70,
            glyph: Glyph::Braille(2),
            highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(4)),
        }]);

        let replaced = raster.replace_particle_cells(Arc::clone(&replacement_particle_cells));

        assert!(Arc::ptr_eq(&replaced.static_cells, &static_cells));
        assert!(Arc::ptr_eq(
            &replaced.particle_cells,
            &replacement_particle_cells
        ));
        assert_eq!(
            replaced.collected_cells(),
            vec![
                replacement_particle_cells[0],
                static_cells[0],
                static_cells[1],
            ]
        );
    }

    #[test]
    fn project_particle_overlay_cells_matches_legacy_particle_overlay_projection() {
        let mut render_frame = representative_frame();
        Arc::make_mut(&mut render_frame.static_config).particles_over_text = false;
        Arc::make_mut(&mut render_frame.static_config).particle_max_lifetime = 2.0;
        Arc::make_mut(&mut render_frame.static_config).particle_switch_octant_braille = 0.4;
        render_frame.set_particles(Arc::new(vec![
            Particle {
                position: Point {
                    row: 10.1,
                    col: 12.1,
                },
                velocity: Point::ZERO,
                lifetime: 1.6,
            },
            Particle {
                position: Point {
                    row: 10.1,
                    col: 12.6,
                },
                velocity: Point::ZERO,
                lifetime: 1.2,
            },
            Particle {
                position: Point {
                    row: 11.1,
                    col: 13.1,
                },
                velocity: Point::ZERO,
                lifetime: 0.2,
            },
            Particle {
                position: Point {
                    row: 11.3,
                    col: 13.1,
                },
                velocity: Point::ZERO,
                lifetime: 0.2,
            },
            Particle {
                position: Point {
                    row: 10.1,
                    col: 10.1,
                },
                velocity: Point::ZERO,
                lifetime: 1.1,
            },
        ]));
        let frame = PlannerFrame::from_render_frame(&render_frame);

        let viewport = Viewport {
            max_row: 20,
            max_col: 20,
        };
        let probe = BackgroundProbeBatch::from_allowed_mask(
            ViewportSnapshot::new(CursorRow(20), CursorCol(20)),
            {
                let mut allowed = vec![false; 400];
                allowed[usize::from(9_u16) * usize::from(20_u16) + usize::from(11_u16)] = true;
                allowed[usize::from(10_u16) * usize::from(20_u16) + usize::from(12_u16)] = true;
                allowed
            },
        );

        let legacy_particle_cells = project_render_plan(
            &crate::draw::render_plan::particle_overlay_plan(&frame, viewport),
            viewport,
            Some(&probe),
        )
        .into_particle_cells();

        assert_eq!(
            project_particle_overlay_cells(&frame, viewport, Some(&probe)),
            legacy_particle_cells,
        );
    }

    #[test]
    fn project_cell_ops_to_spans_preserves_the_merged_span_payload_hash() {
        let cells = [
            CellOp {
                row: 6,
                col: 8,
                zindex: 12,
                glyph: Glyph::BLOCK,
                highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(2)),
            },
            CellOp {
                row: 6,
                col: 9,
                zindex: 12,
                glyph: Glyph::Braille(3),
                highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(4)),
            },
        ];
        let expected_chunks = vec![
            RealizationSpanChunk::new(cells[0].glyph, cells[0].highlight),
            RealizationSpanChunk::new(cells[1].glyph, cells[1].highlight),
        ];

        let spans = project_cell_ops_to_spans(cells.iter());

        assert_eq!(
            spans.as_ref(),
            &[RealizationSpan {
                row: 6,
                col: 8,
                width: 2,
                zindex: 12,
                payload_hash: RealizationSpan::payload_hash_for_chunks(&expected_chunks),
                chunks: Arc::from(expected_chunks),
            }]
        );
    }
}

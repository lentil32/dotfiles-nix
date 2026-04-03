//! Shell-facing realization helpers for scene patches and render plans.
//!
//! This layer normalizes logical planner output into the draw, clear, and noop
//! payloads that the host bridge can apply, keeping missing-basis failures as
//! explicit lifecycle results instead of hidden exceptions.

use crate::core::state::BackgroundProbeBatch;
#[cfg(test)]
use crate::core::state::{ProjectionSnapshot, ScenePatch, ScenePatchKind};
use crate::draw::render_plan::{
    CellOp, ClearOp, Glyph, HighlightRef, RenderPlan, TargetCellOverlay, Viewport,
};
use crate::types::{RenderFrame, ScreenCell, StaticRenderConfig};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
#[cfg(test)]
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PaletteSpec {
    mode: String,
    static_config: Arc<StaticRenderConfig>,
    color_at_cursor: Option<u32>,
}

impl PaletteSpec {
    pub(crate) fn from_frame(frame: &RenderFrame) -> Self {
        Self {
            mode: frame.mode.clone(),
            static_config: Arc::clone(&frame.static_config),
            color_at_cursor: frame.color_at_cursor.clone(),
        }
    }

    pub(crate) fn mode(&self) -> &str {
        &self.mode
    }

    pub(crate) fn cursor_color(&self) -> Option<&str> {
        self.static_config.cursor_color.as_deref()
    }

    pub(crate) fn cursor_color_insert_mode(&self) -> Option<&str> {
        self.static_config.cursor_color_insert_mode.as_deref()
    }

    pub(crate) fn normal_bg(&self) -> Option<&str> {
        self.static_config.normal_bg.as_deref()
    }

    pub(crate) fn transparent_bg_fallback_color(&self) -> &str {
        &self.static_config.transparent_bg_fallback_color
    }

    pub(crate) fn cterm_cursor_colors(&self) -> Option<&[u16]> {
        self.static_config.cterm_cursor_colors.as_deref()
    }

    pub(crate) fn cterm_bg(&self) -> Option<u16> {
        self.static_config.cterm_bg
    }

    pub(crate) fn color_levels(&self) -> u32 {
        self.static_config.color_levels.max(1)
    }

    pub(crate) fn gamma(&self) -> f64 {
        self.static_config.gamma
    }

    pub(crate) fn gamma_bits(&self) -> u64 {
        self.static_config.gamma.to_bits()
    }

    pub(crate) const fn color_at_cursor(&self) -> Option<u32> {
        self.color_at_cursor
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct LogicalRaster {
    clear: Option<ClearOp>,
    cells: Arc<[CellOp]>,
}

impl LogicalRaster {
    pub(crate) fn new(clear: Option<ClearOp>, cells: Arc<[CellOp]>) -> Self {
        Self { clear, cells }
    }

    pub(crate) const fn clear(&self) -> Option<ClearOp> {
        self.clear
    }

    pub(crate) fn cells(&self) -> &[CellOp] {
        self.cells.as_ref()
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

    fn new(row: i64, col: i64, width: u32, zindex: u32, chunks: Vec<RealizationSpanChunk>) -> Self {
        let payload_hash = Self::payload_hash_for_chunks(&chunks);
        Self {
            row,
            col,
            width,
            zindex,
            payload_hash,
            chunks: Arc::from(chunks),
        }
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
    spans: Arc<[RealizationSpan]>,
}

impl RealizationProjection {
    pub(crate) fn new(clear: Option<ClearOp>, spans: Arc<[RealizationSpan]>) -> Self {
        Self { clear, spans }
    }

    pub(crate) const fn clear(&self) -> Option<ClearOp> {
        self.clear
    }

    pub(crate) fn spans(&self) -> &[RealizationSpan] {
        self.spans.as_ref()
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
}

impl RealizationSpanBuilder {
    fn flush_pending(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };

        self.spans.push(RealizationSpan::new(
            pending.row,
            pending.col,
            pending.width,
            pending.zindex,
            pending.chunks,
        ));
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
            });
        }

        let Some(pending) = self.pending.as_mut() else {
            return;
        };
        pending.width = pending.width.saturating_add(1);
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
        glyph: Glyph::BLOCK,
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

pub(crate) fn realize_logical_raster(raster: &LogicalRaster) -> RealizationProjection {
    RealizationProjection::new(
        raster.clear(),
        project_cell_ops_to_spans(raster.cells().iter()),
    )
}

pub(crate) fn project_render_plan(
    plan: &RenderPlan,
    viewport: Viewport,
    background_probe: Option<&BackgroundProbeBatch>,
) -> LogicalRaster {
    let mut linear_cells =
        Vec::<CellOp>::with_capacity(plan.cell_ops.len() + plan.particle_ops.len() + 1);

    for op in &plan.particle_ops {
        if !in_bounds(viewport, op.cell.row, op.cell.col) {
            continue;
        }
        if op.requires_background_probe {
            let Some(screen_cell) = ScreenCell::new(op.cell.row, op.cell.col) else {
                continue;
            };
            if background_probe.is_some_and(|probe| probe.allows_particle(screen_cell)) {
                linear_cells.push(op.cell);
            }
        } else {
            linear_cells.push(op.cell);
        }
    }

    linear_cells.extend(
        plan.cell_ops
            .iter()
            .filter(|op| in_bounds(viewport, op.row, op.col))
            .copied(),
    );

    if let Some(overlay) = plan.target_cell_overlay
        && in_bounds(viewport, overlay.row, overlay.col)
    {
        linear_cells.push(overlay_cell(overlay));
    }

    // background-dependent particle admission is resolved from the explicit
    // observation probe before snapshot retention. The snapshot keeps only logical
    // raster cells, and shell apply derives spans later.
    LogicalRaster::new(plan.clear, Arc::from(linear_cells))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::{PatchBasis, ProjectionSnapshot, ProjectionWitness, ScenePatch};
    use crate::core::types::StrokeId;
    use crate::core::types::{
        CursorCol, CursorRow, IngressSeq, ObservationId, ProjectorRevision, SceneRevision,
        ViewportSnapshot,
    };
    use crate::draw::render_plan::{
        HighlightLevel, ParticleOp, PlannerState, render_frame_to_plan,
    };
    use crate::types::{
        BASE_TIME_INTERVAL, Point, RenderFrame, RenderStepSample, StaticRenderConfig,
    };
    use std::sync::Arc;

    fn cell(row: i64, col: i64, zindex: u32) -> CellOp {
        CellOp {
            row,
            col,
            zindex,
            glyph: Glyph::BLOCK,
            highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(2)),
        }
    }

    fn projection_snapshot(max_kept_windows: usize) -> ProjectionSnapshot {
        ProjectionSnapshot::new(
            ProjectionWitness::new(
                SceneRevision::INITIAL,
                ObservationId::from_ingress_seq(IngressSeq::new(1)),
                ViewportSnapshot::new(CursorRow(20), CursorCol(40)),
                ProjectorRevision::CURRENT,
            ),
            LogicalRaster::new(
                Some(ClearOp { max_kept_windows }),
                Arc::from(Vec::<CellOp>::new()),
            ),
        )
    }

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
            mode: "n".to_string(),
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
            particles: Vec::new().into(),
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

    #[test]
    fn project_render_plan_batches_visible_cells_and_overlay() {
        let plan = RenderPlan {
            cell_ops: vec![cell(4, 5, 20), cell(4, 6, 20)],
            particle_ops: vec![ParticleOp {
                cell: cell(4, 4, 19),
                requires_background_probe: false,
            }],
            target_cell_overlay: Some(TargetCellOverlay {
                row: 4,
                col: 7,
                zindex: 20,
                level: HighlightLevel::from_raw_clamped(2),
            }),
            clear: Some(ClearOp {
                max_kept_windows: 16,
            }),
        };

        let raster = project_render_plan(
            &plan,
            Viewport {
                max_row: 20,
                max_col: 20,
            },
            None,
        );
        let projection = realize_logical_raster(&raster);

        assert_eq!(
            raster.clear(),
            Some(ClearOp {
                max_kept_windows: 16,
            })
        );
        assert_eq!(
            raster.cells(),
            &[
                cell(4, 4, 19),
                cell(4, 5, 20),
                cell(4, 6, 20),
                cell(4, 7, 20)
            ]
        );
        assert_eq!(projection.spans().len(), 2);
        assert_eq!(projection.spans()[0].row(), 4);
        assert_eq!(projection.spans()[0].col(), 4);
        assert_eq!(projection.spans()[0].width(), 1);
        assert_eq!(projection.spans()[1].col(), 5);
        assert_eq!(projection.spans()[1].width(), 3);
    }

    #[test]
    fn project_render_plan_skips_probe_required_particles_without_background_allowance() {
        let plan = RenderPlan {
            particle_ops: vec![
                ParticleOp {
                    cell: cell(5, 8, 11),
                    requires_background_probe: true,
                },
                ParticleOp {
                    cell: cell(0, 8, 11),
                    requires_background_probe: true,
                },
            ],
            ..RenderPlan::default()
        };

        let raster = project_render_plan(
            &plan,
            Viewport {
                max_row: 10,
                max_col: 10,
            },
            None,
        );

        assert!(raster.cells().is_empty());
    }

    #[test]
    fn project_render_plan_materializes_only_background_allowed_probe_particles() {
        let plan = RenderPlan {
            particle_ops: vec![
                ParticleOp {
                    cell: cell(5, 8, 11),
                    requires_background_probe: true,
                },
                ParticleOp {
                    cell: cell(5, 9, 11),
                    requires_background_probe: true,
                },
            ],
            ..RenderPlan::default()
        };
        let mut allowed_mask = vec![false; 100];
        allowed_mask[47] = true;
        let background = BackgroundProbeBatch::from_allowed_mask(
            ViewportSnapshot::new(CursorRow(10), CursorCol(10)),
            allowed_mask,
        );
        let raster = project_render_plan(
            &plan,
            Viewport {
                max_row: 10,
                max_col: 10,
            },
            Some(&background),
        );
        let realized = realize_logical_raster(&raster);

        assert_eq!(raster.cells(), &[cell(5, 8, 11)]);
        assert_eq!(realized.spans().len(), 1);
        assert_eq!(realized.spans()[0].row(), 5);
        assert_eq!(realized.spans()[0].col(), 8);
        assert_eq!(realized.spans()[0].width(), 1);
    }

    #[test]
    fn realization_projection_preserves_legacy_render_plan_cells_for_representative_frame() {
        let frame = representative_frame();
        let viewport = Viewport {
            max_row: 40,
            max_col: 80,
        };
        let planner_output = render_frame_to_plan(&frame, PlannerState::default(), viewport);
        let raster = project_render_plan(&planner_output.plan, viewport, None);
        let realized = realize_logical_raster(&raster);

        assert_eq!(
            flatten_projection_cells(&realized),
            legacy_visible_cells_without_probe(&planner_output.plan, viewport)
        );
    }

    #[test]
    fn project_scene_patch_maps_replace_patch_to_target_snapshot() {
        let target = projection_snapshot(12);
        let patch = ScenePatch::derive(PatchBasis::new(None, Some(target.clone())));

        let projected = project_scene_patch(&patch).expect("replace patch should project");

        assert_eq!(projected, ScenePatchRealization::Draw(&target));
    }

    #[test]
    fn project_scene_patch_maps_clear_and_noop_patch_kinds() {
        let snapshot = projection_snapshot(8);
        let clear_patch = ScenePatch::derive(PatchBasis::new(Some(snapshot.clone()), None));
        let noop_patch =
            ScenePatch::derive(PatchBasis::new(Some(snapshot.clone()), Some(snapshot)));

        assert_eq!(
            project_scene_patch(&clear_patch),
            Ok(ScenePatchRealization::Clear)
        );
        assert_eq!(
            project_scene_patch(&noop_patch),
            Ok(ScenePatchRealization::Noop)
        );
    }

    #[test]
    fn scene_patch_replaces_when_projection_witness_drifts_even_if_raster_is_unchanged() {
        let acknowledged = projection_snapshot(8);
        let target = acknowledged.clone().with_witness(ProjectionWitness::new(
            SceneRevision::INITIAL,
            ObservationId::from_ingress_seq(IngressSeq::new(2)),
            ViewportSnapshot::new(CursorRow(20), CursorCol(40)),
            ProjectorRevision::CURRENT,
        ));

        assert_ne!(acknowledged, target);
        assert_eq!(
            ScenePatch::derive(PatchBasis::new(Some(acknowledged), Some(target))).kind(),
            ScenePatchKind::Replace
        );
    }
}

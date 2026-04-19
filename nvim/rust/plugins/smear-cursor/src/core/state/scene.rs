use crate::core::realization::LogicalRaster;
use crate::core::realization::RealizationProjection;
use crate::core::realization::realize_logical_raster;
use crate::core::realization::realize_particle_cells;
use crate::core::runtime_reducer::TargetCellPresentation;
use crate::core::types::ObservationId;
use crate::core::types::ProjectorRevision;
use crate::core::types::SceneRevision;
use crate::core::types::StepIndex;
use crate::core::types::StrokeId;
use crate::core::types::ViewportSnapshot;
use crate::draw::render_plan::PlannerState as ProjectionPlannerState;
use crate::state::RuntimeState;
use crate::types::ModeClass;
use crate::types::PlannerFrame;
use crate::types::PlannerRenderConfig;
use crate::types::Point;
use crate::types::RenderFrame;
use crate::types::SharedAggregatedParticleCells;
use crate::types::SharedRenderStepSamples;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) enum SemanticEntityId {
    CursorTrail,
}

impl SemanticEntityId {}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CursorTrailProjectionPolicy {
    hide_target_hack: bool,
    max_kept_windows: usize,
    never_draw_over_target: bool,
    particle_max_lifetime: f64,
    particle_switch_octant_braille: f64,
    particles_over_text: bool,
    color_levels: u32,
    block_aspect_ratio: f64,
    tail_duration_ms: f64,
    simulation_hz: f64,
    trail_thickness: f64,
    trail_thickness_x: f64,
    spatial_coherence_weight: f64,
    temporal_stability_weight: f64,
    top_k_per_cell: u8,
    windows_zindex: u32,
}

impl CursorTrailProjectionPolicy {
    pub(crate) fn from_render_frame(frame: &RenderFrame) -> Self {
        Self {
            hide_target_hack: frame.hide_target_hack,
            max_kept_windows: frame.max_kept_windows,
            never_draw_over_target: frame.never_draw_over_target,
            particle_max_lifetime: frame.particle_max_lifetime,
            particle_switch_octant_braille: frame.particle_switch_octant_braille,
            particles_over_text: frame.particles_over_text,
            color_levels: frame.color_levels,
            block_aspect_ratio: frame.block_aspect_ratio,
            tail_duration_ms: frame.tail_duration_ms,
            simulation_hz: frame.simulation_hz,
            trail_thickness: frame.trail_thickness,
            trail_thickness_x: frame.trail_thickness_x,
            spatial_coherence_weight: frame.spatial_coherence_weight,
            temporal_stability_weight: frame.temporal_stability_weight,
            top_k_per_cell: frame.top_k_per_cell,
            windows_zindex: frame.windows_zindex,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CursorTrailGeometry {
    mode: ModeClass,
    corners: [Point; 4],
    step_samples: SharedRenderStepSamples,
    planner_idle_steps: u32,
    target: Point,
    target_corners: [Point; 4],
    vertical_bar: bool,
    trail_stroke_id: StrokeId,
    retarget_epoch: u64,
    particle_count: usize,
    aggregated_particle_cells: SharedAggregatedParticleCells,
}

impl CursorTrailGeometry {
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
        }
    }

    fn planner_config(&self, policy: &CursorTrailProjectionPolicy) -> PlannerRenderConfig {
        PlannerRenderConfig {
            hide_target_hack: policy.hide_target_hack,
            max_kept_windows: policy.max_kept_windows,
            never_draw_over_target: policy.never_draw_over_target,
            particle_max_lifetime: policy.particle_max_lifetime,
            particle_switch_octant_braille: policy.particle_switch_octant_braille,
            particles_over_text: policy.particles_over_text,
            color_levels: policy.color_levels,
            block_aspect_ratio: policy.block_aspect_ratio,
            tail_duration_ms: policy.tail_duration_ms,
            simulation_hz: policy.simulation_hz,
            trail_thickness: policy.trail_thickness,
            trail_thickness_x: policy.trail_thickness_x,
            spatial_coherence_weight: policy.spatial_coherence_weight,
            temporal_stability_weight: policy.temporal_stability_weight,
            top_k_per_cell: policy.top_k_per_cell,
            windows_zindex: policy.windows_zindex,
        }
    }

    pub(crate) fn planner_frame(&self, policy: &CursorTrailProjectionPolicy) -> PlannerFrame {
        // phase 3 keeps palette resolution out of semantic identity, so the projector
        // rebuilds a planner-only frame from semantic geometry plus explicit projection policy
        // instead of retaining planner config inside semantic identity.
        PlannerFrame {
            mode: self.mode,
            corners: self.corners,
            step_samples: self.step_samples.clone(),
            planner_idle_steps: self.planner_idle_steps,
            target: self.target,
            target_corners: self.target_corners,
            vertical_bar: self.vertical_bar,
            trail_stroke_id: self.trail_stroke_id,
            retarget_epoch: self.retarget_epoch,
            particle_count: self.particle_count,
            aggregated_particle_cells: self.aggregated_particle_cells.clone(),
            planner_config: self.planner_config(policy),
        }
    }

    pub(crate) fn requires_background_probe(&self, policy: &CursorTrailProjectionPolicy) -> bool {
        !policy.particles_over_text && self.particle_count > 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CursorTrailSemantic {
    geometry: CursorTrailGeometry,
    target_cell_presentation: TargetCellPresentation,
}

impl CursorTrailSemantic {
    // target-cell presentation is currently derivable from the frame inputs, but
    // projection identity depends on it as an explicit reducer fact rather than an implicit
    // render-plan convention.
    pub(crate) fn new(
        geometry: CursorTrailGeometry,
        target_cell_presentation: TargetCellPresentation,
    ) -> Self {
        Self {
            geometry,
            target_cell_presentation,
        }
    }

    pub(crate) fn from_render_frame(
        frame: &RenderFrame,
        target_cell_presentation: TargetCellPresentation,
    ) -> Self {
        Self::new(
            CursorTrailGeometry::from_render_frame(frame),
            target_cell_presentation,
        )
    }

    pub(crate) const fn geometry(&self) -> &CursorTrailGeometry {
        &self.geometry
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SemanticEntity {
    CursorTrail(CursorTrailSemantic),
}

impl SemanticEntity {
    pub(crate) const fn id(&self) -> SemanticEntityId {
        match self {
            Self::CursorTrail(_) => SemanticEntityId::CursorTrail,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SemanticScene {
    entities: BTreeMap<SemanticEntityId, SemanticEntity>,
}

impl SemanticScene {
    pub(crate) fn entity(&self, id: SemanticEntityId) -> Option<&SemanticEntity> {
        self.entities.get(&id)
    }

    pub(crate) fn with_entity(mut self, entity: SemanticEntity) -> Self {
        self.entities.insert(entity.id(), entity);
        self
    }

    pub(crate) fn without_entity(mut self, id: SemanticEntityId) -> Self {
        self.entities.remove(&id);
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct DirtyEntitySet {
    entities: BTreeSet<SemanticEntityId>,
}

impl DirtyEntitySet {
    #[cfg(test)]
    pub(crate) const fn entities(&self) -> &BTreeSet<SemanticEntityId> {
        &self.entities
    }

    pub(crate) fn insert(mut self, entity_id: SemanticEntityId) -> Self {
        self.entities.insert(entity_id);
        self
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct ProjectionWitness {
    scene_revision: SceneRevision,
    observation_id: ObservationId,
    viewport: ViewportSnapshot,
    projector_revision: ProjectorRevision,
}

impl ProjectionWitness {
    pub(crate) const fn new(
        scene_revision: SceneRevision,
        observation_id: ObservationId,
        viewport: ViewportSnapshot,
        projector_revision: ProjectorRevision,
    ) -> Self {
        Self {
            scene_revision,
            observation_id,
            viewport,
            projector_revision,
        }
    }

    pub(crate) const fn scene_revision(self) -> SceneRevision {
        self.scene_revision
    }

    pub(crate) const fn observation_id(self) -> ObservationId {
        self.observation_id
    }

    pub(crate) const fn viewport(self) -> ViewportSnapshot {
        self.viewport
    }

    pub(crate) const fn projector_revision(self) -> ProjectorRevision {
        self.projector_revision
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProjectionReuseKey {
    trail_signature: Option<u64>,
    particle_overlay_signature: Option<u64>,
    planner_clock: Option<ProjectionPlannerClock>,
    target_cell_presentation: TargetCellPresentation,
    policy: CursorTrailProjectionPolicy,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct ProjectionPlannerClock {
    step_index: StepIndex,
    history_revision: u64,
}

impl ProjectionPlannerClock {
    pub(crate) const fn new(step_index: StepIndex, history_revision: u64) -> Self {
        Self {
            step_index,
            history_revision,
        }
    }
}

impl ProjectionReuseKey {
    pub(crate) fn new(
        trail_signature: Option<u64>,
        particle_overlay_signature: Option<u64>,
        planner_clock: Option<ProjectionPlannerClock>,
        target_cell_presentation: TargetCellPresentation,
        policy: CursorTrailProjectionPolicy,
    ) -> Self {
        Self {
            trail_signature,
            particle_overlay_signature,
            planner_clock,
            target_cell_presentation,
            policy,
        }
    }

    pub(crate) const fn trail_signature(&self) -> Option<u64> {
        self.trail_signature
    }

    pub(crate) const fn particle_overlay_signature(&self) -> Option<u64> {
        self.particle_overlay_signature
    }

    pub(crate) const fn planner_clock(&self) -> Option<ProjectionPlannerClock> {
        self.planner_clock
    }

    pub(crate) const fn target_cell_presentation(&self) -> TargetCellPresentation {
        self.target_cell_presentation
    }

    pub(crate) const fn policy(&self) -> &CursorTrailProjectionPolicy {
        &self.policy
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProjectionSnapshot {
    witness: ProjectionWitness,
    logical_raster: Arc<LogicalRaster>,
    realization: RealizationProjection,
}

impl ProjectionSnapshot {
    fn from_parts(
        witness: ProjectionWitness,
        logical_raster: Arc<LogicalRaster>,
        realization: RealizationProjection,
    ) -> Self {
        Self {
            witness,
            logical_raster,
            realization,
        }
    }

    pub(crate) fn new(witness: ProjectionWitness, logical_raster: LogicalRaster) -> Self {
        let realization = realize_logical_raster(&logical_raster);
        Self::from_parts(witness, Arc::new(logical_raster), realization)
    }

    pub(crate) const fn witness(&self) -> ProjectionWitness {
        self.witness
    }

    pub(crate) fn logical_raster(&self) -> &LogicalRaster {
        self.logical_raster.as_ref()
    }

    pub(crate) fn realization(&self) -> &RealizationProjection {
        &self.realization
    }

    pub(crate) fn rebind_witness(&self, witness: ProjectionWitness) -> Self {
        Self::from_parts(
            witness,
            Arc::clone(&self.logical_raster),
            self.realization.clone(),
        )
    }

    pub(crate) fn with_replaced_particle_cells(
        &self,
        witness: ProjectionWitness,
        particle_cells: Arc<[crate::draw::render_plan::CellOp]>,
    ) -> Self {
        let logical_raster = Arc::new(self.logical_raster().replace_particle_cells(particle_cells));
        let realization = self
            .realization
            .replace_particle_spans(realize_particle_cells(logical_raster.particle_cells()));
        Self::from_parts(witness, logical_raster, realization)
    }

    pub(crate) fn same_render_output_as(&self, other: &Self) -> bool {
        // shell redraw authority is observation-bound, not just raster-bound.
        // If the reducer accepted a new projection witness, the shell must treat that as
        // replace work so animation can redraw after jumps, window changes, or other
        // observation churn even when the retained raster happens to be identical.
        self == other
    }

    pub(crate) fn with_witness(mut self, witness: ProjectionWitness) -> Self {
        self.witness = witness;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProjectionCacheEntry {
    planner_state: ProjectionPlannerState,
    snapshot: ProjectionSnapshot,
    reuse_key: ProjectionReuseKey,
}

impl ProjectionCacheEntry {
    pub(crate) const fn new(
        planner_state: ProjectionPlannerState,
        snapshot: ProjectionSnapshot,
        reuse_key: ProjectionReuseKey,
    ) -> Self {
        Self {
            planner_state,
            snapshot,
            reuse_key,
        }
    }

    pub(crate) const fn planner_state(&self) -> &ProjectionPlannerState {
        &self.planner_state
    }

    pub(crate) const fn snapshot(&self) -> &ProjectionSnapshot {
        &self.snapshot
    }

    pub(crate) const fn reuse_key(&self) -> &ProjectionReuseKey {
        &self.reuse_key
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) enum ProjectionCache {
    #[default]
    Invalid,
    Ready(Box<ProjectionCacheEntry>),
}

impl ProjectionCache {
    pub(crate) fn entry(&self) -> Option<&ProjectionCacheEntry> {
        match self {
            Self::Invalid => None,
            Self::Ready(entry) => Some(entry.as_ref()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PlannedProjectionUpdate {
    Keep,
    Replace(ProjectionCache),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedSceneUpdate {
    revision: SceneRevision,
    semantics: SemanticScene,
    projection: PlannedProjectionUpdate,
    dirty: DirtyEntitySet,
}

impl PlannedSceneUpdate {
    pub(crate) fn new(
        revision: SceneRevision,
        semantics: SemanticScene,
        projection: PlannedProjectionUpdate,
        dirty: DirtyEntitySet,
    ) -> Self {
        Self {
            revision,
            semantics,
            projection,
            dirty,
        }
    }

    pub(crate) const fn revision(&self) -> SceneRevision {
        self.revision
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PatchBasis {
    acknowledged: Option<ProjectionSnapshot>,
    target: Option<ProjectionSnapshot>,
}

impl PatchBasis {
    pub(crate) fn new(
        acknowledged: Option<ProjectionSnapshot>,
        target: Option<ProjectionSnapshot>,
    ) -> Self {
        Self {
            acknowledged,
            target,
        }
    }

    pub(crate) const fn acknowledged(&self) -> Option<&ProjectionSnapshot> {
        self.acknowledged.as_ref()
    }

    pub(crate) const fn target(&self) -> Option<&ProjectionSnapshot> {
        self.target.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScenePatch {
    basis: PatchBasis,
    kind: ScenePatchKind,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ScenePatchKind {
    Noop,
    Clear,
    Replace,
}

impl ScenePatchKind {
    pub(crate) fn from_basis(basis: &PatchBasis) -> Self {
        match (basis.acknowledged(), basis.target()) {
            (None, None) => Self::Noop,
            (Some(acknowledged), Some(target)) if acknowledged.same_render_output_as(target) => {
                Self::Noop
            }
            (_, None) => Self::Clear,
            _ => {
                // phase 4 keeps patch shape intentionally coarse. The authoritative basis
                // is explicit now; phase 5 can refine replace work into realization-specific
                // projection without reopening protocol ownership.
                Self::Replace
            }
        }
    }
}

impl ScenePatch {
    pub(crate) fn derive(basis: PatchBasis) -> Self {
        let kind = ScenePatchKind::from_basis(&basis);
        Self { basis, kind }
    }

    pub(crate) const fn basis(&self) -> &PatchBasis {
        &self.basis
    }

    pub(crate) const fn kind(&self) -> ScenePatchKind {
        self.kind
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SceneState {
    revision: SceneRevision,
    semantics: SemanticScene,
    // the motion model is now scene-owned render truth rather than a sibling authority
    // beside the semantic scene.
    motion: RuntimeState,
    projection: ProjectionCache,
    dirty: DirtyEntitySet,
}

impl SceneState {
    pub(crate) const fn revision(&self) -> SceneRevision {
        self.revision
    }

    pub(crate) const fn semantics(&self) -> &SemanticScene {
        &self.semantics
    }

    pub(crate) const fn motion(&self) -> &RuntimeState {
        &self.motion
    }

    pub(crate) fn motion_mut(&mut self) -> &mut RuntimeState {
        &mut self.motion
    }

    pub(crate) fn take_motion(&mut self) -> RuntimeState {
        std::mem::take(&mut self.motion)
    }

    #[cfg(test)]
    pub(crate) const fn dirty(&self) -> &DirtyEntitySet {
        &self.dirty
    }

    pub(crate) fn projection_entry(&self) -> Option<&ProjectionCacheEntry> {
        self.projection.entry()
    }

    pub(crate) fn apply_planned_update(&mut self, update: PlannedSceneUpdate) {
        let PlannedSceneUpdate {
            revision,
            semantics,
            projection,
            dirty,
        } = update;
        self.revision = revision;
        self.semantics = semantics;
        if let PlannedProjectionUpdate::Replace(projection) = projection {
            self.projection = projection;
        }
        self.dirty = dirty;
    }

    #[cfg(test)]
    pub(crate) fn with_semantics(mut self, semantics: SemanticScene) -> Self {
        self.semantics = semantics;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_projection(mut self, projection: ProjectionCache) -> Self {
        self.projection = projection;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::ProjectionSnapshot;
    use super::ProjectionWitness;
    use crate::core::realization::LogicalRaster;
    use crate::core::realization::realize_logical_raster;
    use crate::core::types::CursorCol;
    use crate::core::types::CursorRow;
    use crate::core::types::IngressSeq;
    use crate::core::types::ObservationId;
    use crate::core::types::ProjectorRevision;
    use crate::core::types::SceneRevision;
    use crate::core::types::ViewportSnapshot;
    use crate::draw::render_plan::CellOp;
    use crate::draw::render_plan::ClearOp;
    use crate::draw::render_plan::Glyph;
    use crate::draw::render_plan::HighlightLevel;
    use crate::draw::render_plan::HighlightRef;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    fn projection_witness() -> ProjectionWitness {
        ProjectionWitness::new(
            SceneRevision::INITIAL,
            ObservationId::from_ingress_seq(IngressSeq::new(7)),
            ViewportSnapshot::new(CursorRow(40), CursorCol(120)),
            ProjectorRevision::CURRENT,
        )
    }

    #[test]
    fn projection_snapshot_caches_realization_for_the_retained_logical_raster() {
        let raster = LogicalRaster::new(
            Some(ClearOp {
                max_kept_windows: 4,
            }),
            Arc::from([
                CellOp {
                    row: 3,
                    col: 5,
                    zindex: 9,
                    glyph: Glyph::Static("A"),
                    highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(1)),
                },
                CellOp {
                    row: 3,
                    col: 6,
                    zindex: 9,
                    glyph: Glyph::Static("B"),
                    highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(2)),
                },
                CellOp {
                    row: 4,
                    col: 2,
                    zindex: 1,
                    glyph: Glyph::BLOCK,
                    highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(1)),
                },
            ]),
        );
        let snapshot = ProjectionSnapshot::new(projection_witness(), raster);

        assert_eq!(
            snapshot.realization(),
            &realize_logical_raster(snapshot.logical_raster())
        );
    }

    #[test]
    fn replacing_particle_cells_reuses_the_retained_static_realization_segment() {
        let snapshot = ProjectionSnapshot::new(
            projection_witness(),
            LogicalRaster::from_segments(
                Some(ClearOp {
                    max_kept_windows: 4,
                }),
                Arc::from([CellOp {
                    row: 3,
                    col: 5,
                    zindex: 9,
                    glyph: Glyph::Static("P"),
                    highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(1)),
                }]),
                Arc::from([
                    CellOp {
                        row: 4,
                        col: 2,
                        zindex: 1,
                        glyph: Glyph::BLOCK,
                        highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(2)),
                    },
                    CellOp {
                        row: 4,
                        col: 3,
                        zindex: 1,
                        glyph: Glyph::Static("x"),
                        highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(3)),
                    },
                ]),
            ),
        );

        let replaced = snapshot.with_replaced_particle_cells(
            projection_witness(),
            Arc::from([CellOp {
                row: 8,
                col: 9,
                zindex: 12,
                glyph: Glyph::Braille(2),
                highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(4)),
            }]),
        );

        assert_eq!(
            snapshot.realization().static_spans().as_ptr(),
            replaced.realization().static_spans().as_ptr(),
        );
        assert_ne!(
            snapshot.realization().particle_spans(),
            replaced.realization().particle_spans()
        );
        assert_eq!(
            replaced.realization(),
            &realize_logical_raster(replaced.logical_raster())
        );
    }
}

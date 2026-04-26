use crate::core::realization::LogicalRaster;
use crate::core::realization::RealizationProjection;
use crate::core::realization::realize_logical_raster;
use crate::core::realization::realize_particle_cells;
use crate::core::runtime_reducer::TargetCellPresentation;
use crate::core::types::MotionRevision;
use crate::core::types::ObservationId;
use crate::core::types::ProjectionPolicyRevision;
use crate::core::types::ProjectorRevision;
use crate::core::types::RenderRevision;
use crate::core::types::SemanticRevision;
use crate::core::types::StepIndex;
use crate::draw::render_plan::PlannerState as ProjectionPlannerState;
use crate::position::ViewportBounds;
use std::rc::Rc;
use std::sync::Arc;

#[cfg(test)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) enum SemanticEntityId {
    CursorTrail,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CursorTrailSemantic {
    target_cell_presentation: TargetCellPresentation,
}

impl CursorTrailSemantic {
    // target-cell presentation is currently derivable from the frame inputs, but
    // projection identity depends on it as an explicit reducer fact rather than an implicit
    // render-plan convention.
    pub(crate) const fn new(target_cell_presentation: TargetCellPresentation) -> Self {
        Self {
            target_cell_presentation,
        }
    }

    pub(crate) const fn target_cell_presentation(&self) -> TargetCellPresentation {
        self.target_cell_presentation
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SemanticState {
    revision: SemanticRevision,
    cursor_trail: Option<CursorTrailSemantic>,
}

impl SemanticState {
    pub(crate) const fn revision(&self) -> SemanticRevision {
        self.revision
    }

    pub(crate) const fn cursor_trail(&self) -> Option<&CursorTrailSemantic> {
        self.cursor_trail.as_ref()
    }
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct ProjectionWitness {
    render_revision: RenderRevision,
    observation_id: ObservationId,
    viewport: ViewportBounds,
    projector_revision: ProjectorRevision,
}

impl ProjectionWitness {
    pub(crate) fn new(
        render_revision: RenderRevision,
        observation_id: ObservationId,
        viewport: ViewportBounds,
        projector_revision: ProjectorRevision,
    ) -> Self {
        Self {
            render_revision,
            observation_id,
            viewport,
            projector_revision,
        }
    }

    pub(crate) const fn render_revision(self) -> RenderRevision {
        self.render_revision
    }

    pub(crate) const fn observation_id(self) -> ObservationId {
        self.observation_id
    }

    pub(crate) const fn viewport(self) -> ViewportBounds {
        self.viewport
    }

    pub(crate) const fn projector_revision(self) -> ProjectorRevision {
        self.projector_revision
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct ProjectionReuseKey {
    trail_signature: Option<u64>,
    particle_overlay_signature: Option<u64>,
    planner_clock: Option<ProjectionPlannerClock>,
    target_cell_presentation: TargetCellPresentation,
    projection_policy_revision: ProjectionPolicyRevision,
}

impl ProjectionReuseKey {
    pub(crate) fn new(
        trail_signature: Option<u64>,
        particle_overlay_signature: Option<u64>,
        planner_clock: Option<ProjectionPlannerClock>,
        target_cell_presentation: TargetCellPresentation,
        projection_policy_revision: ProjectionPolicyRevision,
    ) -> Self {
        Self {
            trail_signature,
            particle_overlay_signature,
            planner_clock,
            target_cell_presentation,
            projection_policy_revision,
        }
    }

    pub(crate) const fn trail_signature(self) -> Option<u64> {
        self.trail_signature
    }

    pub(crate) const fn particle_overlay_signature(self) -> Option<u64> {
        self.particle_overlay_signature
    }

    pub(crate) const fn planner_clock(self) -> Option<ProjectionPlannerClock> {
        self.planner_clock
    }

    pub(crate) const fn target_cell_presentation(self) -> TargetCellPresentation {
        self.target_cell_presentation
    }

    pub(crate) const fn projection_policy_revision(self) -> ProjectionPolicyRevision {
        self.projection_policy_revision
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RetainedProjection {
    // snapshot: binds the retained projection to the source revisions and viewport.
    witness: ProjectionWitness,
    // cache: retained planner and raster materialization for that witness.
    reuse_key: ProjectionReuseKey,
    cached_planner_state: ProjectionPlannerState,
    cached_logical_raster: Arc<LogicalRaster>,
    cached_realization: RealizationProjection,
}

impl RetainedProjection {
    fn from_parts(
        witness: ProjectionWitness,
        reuse_key: ProjectionReuseKey,
        cached_planner_state: ProjectionPlannerState,
        cached_logical_raster: Arc<LogicalRaster>,
        cached_realization: RealizationProjection,
    ) -> Self {
        Self {
            witness,
            reuse_key,
            cached_planner_state,
            cached_logical_raster,
            cached_realization,
        }
    }

    pub(crate) fn new(
        witness: ProjectionWitness,
        reuse_key: ProjectionReuseKey,
        cached_planner_state: ProjectionPlannerState,
        logical_raster: LogicalRaster,
    ) -> Self {
        let realization = realize_logical_raster(&logical_raster);
        Self::from_parts(
            witness,
            reuse_key,
            cached_planner_state,
            Arc::new(logical_raster),
            realization,
        )
    }

    pub(crate) const fn witness(&self) -> ProjectionWitness {
        self.witness
    }

    pub(crate) const fn reuse_key(&self) -> ProjectionReuseKey {
        self.reuse_key
    }

    pub(crate) const fn cached_planner_state(&self) -> &ProjectionPlannerState {
        &self.cached_planner_state
    }

    pub(crate) fn cached_logical_raster(&self) -> &LogicalRaster {
        self.cached_logical_raster.as_ref()
    }

    pub(crate) fn cached_realization(&self) -> &RealizationProjection {
        &self.cached_realization
    }

    pub(crate) fn rebind_snapshot(
        &self,
        witness: ProjectionWitness,
        reuse_key: ProjectionReuseKey,
    ) -> Self {
        Self::from_parts(
            witness,
            reuse_key,
            self.cached_planner_state.clone(),
            Arc::clone(&self.cached_logical_raster),
            self.cached_realization.clone(),
        )
    }

    pub(crate) fn with_replaced_particle_cells(
        &self,
        witness: ProjectionWitness,
        reuse_key: ProjectionReuseKey,
        particle_cells: Arc<[crate::draw::render_plan::CellOp]>,
    ) -> Self {
        let cached_logical_raster = Arc::new(
            self.cached_logical_raster()
                .replace_particle_cells(particle_cells),
        );
        let cached_realization =
            self.cached_realization
                .replace_particle_spans(realize_particle_cells(
                    cached_logical_raster.particle_cells(),
                ));
        Self::from_parts(
            witness,
            reuse_key,
            self.cached_planner_state.clone(),
            cached_logical_raster,
            cached_realization,
        )
    }

    pub(crate) fn into_handle(self) -> ProjectionHandle {
        ProjectionHandle::new(self)
    }

    #[cfg(debug_assertions)]
    fn debug_assert_invariants(&self) {
        let expected_realization = realize_logical_raster(self.cached_logical_raster());
        debug_assert_eq!(
            self.cached_realization, expected_realization,
            "retained projection shell materialization must match the retained logical raster"
        );
    }

    #[cfg(not(debug_assertions))]
    fn debug_assert_invariants(&self) {}

    #[cfg(test)]
    pub(crate) fn with_witness(mut self, witness: ProjectionWitness) -> Self {
        self.witness = witness;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_cached_realization_for_test(
        mut self,
        cached_realization: RealizationProjection,
    ) -> Self {
        self.cached_realization = cached_realization;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProjectionHandle(Arc<RetainedProjection>);

impl ProjectionHandle {
    #[expect(
        clippy::arc_with_non_send_sync,
        reason = "ProjectionHandle is a single-threaded shared ownership handle between scene and realization state; keeping Arc preserves the retained-handle API shape during this migration."
    )]
    pub(crate) fn new(projection: RetainedProjection) -> Self {
        Self(Arc::new(projection))
    }

    pub(in crate::core::state) fn retained(&self) -> &RetainedProjection {
        self.0.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_invariants(&self) {
        self.retained().debug_assert_invariants();
    }

    #[cfg(not(debug_assertions))]
    pub(crate) fn debug_assert_invariants(&self) {}
}

impl From<RetainedProjection> for ProjectionHandle {
    fn from(projection: RetainedProjection) -> Self {
        Self::new(projection)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ProjectionReuseCache {
    // cache: purgeable retained projection reuse state.
    retained_projection: Option<ProjectionHandle>,
}

impl ProjectionReuseCache {
    pub(in crate::core) fn retained_projection(&self) -> Option<&RetainedProjection> {
        self.retained_projection
            .as_ref()
            .map(ProjectionHandle::retained)
    }

    pub(in crate::core) fn retained_projection_handle(&self) -> Option<&ProjectionHandle> {
        self.retained_projection.as_ref()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ProjectionState {
    // authoritative: projection freshness tied to reducer planning output.
    motion_revision: MotionRevision,
    last_motion_fingerprint: Option<u64>,
    // cache: retained projection reuse state.
    cache: ProjectionReuseCache,
}

impl ProjectionState {
    pub(crate) const fn motion_revision(&self) -> MotionRevision {
        self.motion_revision
    }

    pub(crate) const fn last_motion_fingerprint(&self) -> Option<u64> {
        self.last_motion_fingerprint
    }

    pub(in crate::core) fn retained_projection(&self) -> Option<&RetainedProjection> {
        self.cache.retained_projection()
    }

    pub(in crate::core) fn retained_projection_handle(&self) -> Option<&ProjectionHandle> {
        self.cache.retained_projection_handle()
    }

    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_invariants(&self) {
        if let Some(retained_projection) = self.retained_projection_handle() {
            retained_projection.debug_assert_invariants();
        }
    }

    #[cfg(not(debug_assertions))]
    pub(crate) fn debug_assert_invariants(&self) {}
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PlannedProjectionUpdate {
    Keep,
    Replace(Option<ProjectionHandle>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedSceneUpdate {
    semantic_revision: SemanticRevision,
    motion_revision: MotionRevision,
    last_motion_fingerprint: Option<u64>,
    cursor_trail: Option<CursorTrailSemantic>,
    projection: PlannedProjectionUpdate,
}

impl PlannedSceneUpdate {
    pub(crate) fn new(
        semantic_revision: SemanticRevision,
        motion_revision: MotionRevision,
        last_motion_fingerprint: Option<u64>,
        cursor_trail: Option<CursorTrailSemantic>,
        projection: PlannedProjectionUpdate,
    ) -> Self {
        Self {
            semantic_revision,
            motion_revision,
            last_motion_fingerprint,
            cursor_trail,
            projection,
        }
    }

    pub(crate) const fn render_revision(&self) -> RenderRevision {
        RenderRevision::new(self.motion_revision, self.semantic_revision)
    }

    pub(crate) fn apply_to(self, semantics: &mut SemanticState, projection: &mut ProjectionState) {
        let Self {
            semantic_revision,
            motion_revision,
            last_motion_fingerprint,
            cursor_trail,
            projection: next_retained_projection,
        } = self;
        semantics.revision = semantic_revision;
        semantics.cursor_trail = cursor_trail;
        projection.motion_revision = motion_revision;
        projection.last_motion_fingerprint = last_motion_fingerprint;
        if let PlannedProjectionUpdate::Replace(retained_projection) = next_retained_projection {
            projection.cache.retained_projection = retained_projection;
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PatchBasis {
    acknowledged: Option<ProjectionHandle>,
    target: Option<ProjectionHandle>,
}

impl PatchBasis {
    pub(crate) fn new(
        acknowledged: Option<ProjectionHandle>,
        target: Option<ProjectionHandle>,
    ) -> Self {
        Self {
            acknowledged,
            target,
        }
    }

    pub(crate) fn acknowledged_handle(&self) -> Option<&ProjectionHandle> {
        self.acknowledged.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn target(&self) -> Option<&RetainedProjection> {
        self.target.as_ref().map(ProjectionHandle::retained)
    }

    pub(crate) fn target_handle(&self) -> Option<&ProjectionHandle> {
        self.target.as_ref()
    }

    pub(crate) fn kind(&self) -> ScenePatchKind {
        match (self.acknowledged.as_ref(), self.target.as_ref()) {
            (None, None) => ScenePatchKind::Noop,
            (Some(acknowledged), Some(target)) if acknowledged.same_render_output_as(target) => {
                ScenePatchKind::Noop
            }
            (Some(_), None) => ScenePatchKind::Clear,
            (None, Some(_)) | (Some(_), Some(_)) => ScenePatchKind::Replace,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScenePatch {
    basis: PatchBasis,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ScenePatchKind {
    Noop,
    Clear,
    Replace,
}

impl ScenePatch {
    pub(crate) fn derive(basis: PatchBasis) -> Self {
        Self { basis }
    }

    pub(crate) const fn basis(&self) -> &PatchBasis {
        &self.basis
    }

    pub(crate) fn kind(&self) -> ScenePatchKind {
        self.basis.kind()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SceneState {
    // Non-owner composite view over authoritative semantics plus projection state.
    semantics: Rc<SemanticState>,
    projection: Rc<ProjectionState>,
}

impl SceneState {
    pub(crate) fn from_parts(
        semantics: Rc<SemanticState>,
        projection: Rc<ProjectionState>,
    ) -> Self {
        Self {
            semantics,
            projection,
        }
    }

    pub(crate) fn render_revision(&self) -> RenderRevision {
        RenderRevision::new(self.motion_revision(), self.semantic_revision())
    }

    pub(crate) fn semantic_revision(&self) -> SemanticRevision {
        self.semantics.revision()
    }

    pub(crate) fn motion_revision(&self) -> MotionRevision {
        self.projection.motion_revision()
    }

    pub(crate) fn last_motion_fingerprint(&self) -> Option<u64> {
        self.projection.last_motion_fingerprint()
    }

    pub(crate) fn cursor_trail(&self) -> Option<&CursorTrailSemantic> {
        self.semantics.cursor_trail()
    }

    pub(in crate::core) fn retained_projection(&self) -> Option<&RetainedProjection> {
        self.projection.retained_projection()
    }

    #[cfg(test)]
    pub(in crate::core) fn retained_projection_handle(&self) -> Option<&ProjectionHandle> {
        self.projection.retained_projection_handle()
    }

    #[cfg(test)]
    pub(crate) fn apply_planned_update(&mut self, update: PlannedSceneUpdate) {
        update.apply_to(
            Rc::make_mut(&mut self.semantics),
            Rc::make_mut(&mut self.projection),
        );
    }

    #[cfg(test)]
    pub(crate) fn with_cursor_trail(mut self, cursor_trail: CursorTrailSemantic) -> Self {
        Rc::make_mut(&mut self.semantics).cursor_trail = Some(cursor_trail);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_retained_projection(mut self, projection: ProjectionHandle) -> Self {
        Rc::make_mut(&mut self.projection).cache.retained_projection = Some(projection);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::ProjectionReuseCache;
    use super::ProjectionReuseKey;
    use super::ProjectionState;
    use super::ProjectionWitness;
    use super::RetainedProjection;
    use crate::core::realization::LogicalRaster;
    use crate::core::realization::realize_logical_raster;
    use crate::core::runtime_reducer::TargetCellPresentation;
    use crate::core::types::IngressSeq;
    use crate::core::types::MotionRevision;
    use crate::core::types::ObservationId;
    use crate::core::types::ProjectionPolicyRevision;
    use crate::core::types::ProjectorRevision;
    use crate::core::types::RenderRevision;
    use crate::draw::render_plan::CellOp;
    use crate::draw::render_plan::ClearOp;
    use crate::draw::render_plan::Glyph;
    use crate::draw::render_plan::HighlightLevel;
    use crate::draw::render_plan::HighlightRef;
    use crate::draw::render_plan::PlannerState as ProjectionPlannerState;
    use crate::position::ViewportBounds;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    fn projection_witness_with_ingress_seq(ingress_seq: u64) -> ProjectionWitness {
        ProjectionWitness::new(
            RenderRevision::INITIAL,
            ObservationId::from_ingress_seq(IngressSeq::new(ingress_seq)),
            ViewportBounds::new(40, 120).expect("positive viewport bounds"),
            ProjectorRevision::CURRENT,
        )
    }

    fn projection_reuse_key() -> ProjectionReuseKey {
        ProjectionReuseKey::new(
            None,
            None,
            None,
            TargetCellPresentation::None,
            ProjectionPolicyRevision::INITIAL,
        )
    }

    fn projection_witness() -> ProjectionWitness {
        projection_witness_with_ingress_seq(7)
    }

    fn retained_projection_with_ingress_seq(ingress_seq: u64) -> RetainedProjection {
        RetainedProjection::new(
            projection_witness_with_ingress_seq(ingress_seq),
            projection_reuse_key(),
            ProjectionPlannerState::default(),
            LogicalRaster::new(None, Arc::from([])),
        )
    }

    #[test]
    fn retained_projection_caches_realization_for_the_retained_logical_raster() {
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
        let retained = RetainedProjection::new(
            projection_witness(),
            projection_reuse_key(),
            ProjectionPlannerState::default(),
            raster,
        );

        assert_eq!(
            retained.cached_realization(),
            &realize_logical_raster(retained.cached_logical_raster())
        );
    }

    #[test]
    fn projection_state_debug_assert_rejects_stale_retained_projection_materialization() {
        let stale_realization = realize_logical_raster(&LogicalRaster::new(
            None,
            Arc::from([CellOp {
                row: 9,
                col: 9,
                zindex: 3,
                glyph: Glyph::Static("Q"),
                highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(5)),
            }]),
        ));
        let projection = ProjectionState {
            motion_revision: MotionRevision::INITIAL,
            last_motion_fingerprint: None,
            cache: ProjectionReuseCache {
                retained_projection: Some(
                    retained_projection_with_ingress_seq(7)
                        .with_cached_realization_for_test(stale_realization)
                        .into_handle(),
                ),
            },
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            projection.debug_assert_invariants();
        }));

        if cfg!(debug_assertions) {
            assert!(result.is_err());
        } else {
            assert!(result.is_ok());
        }
    }
}

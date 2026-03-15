use super::{PatchBasis, ProjectionSnapshot, ScenePatch, ScenePatchKind, SceneState};
use crate::core::realization::PaletteSpec;
use crate::core::runtime_reducer::{
    RenderAllocationPolicy, RenderCleanupAction, RenderSideEffects,
};
use crate::core::types::{Millis, ProposalId};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct DegradedApplyMetrics {
    planned_ops: usize,
    applied_ops: usize,
    skipped_ops_capacity: usize,
    reuse_failed_missing_window: usize,
    reuse_failed_reconfigure: usize,
    reuse_failed_missing_buffer: usize,
    windows_recovered: usize,
}

impl DegradedApplyMetrics {
    pub(crate) const fn new(
        planned_ops: usize,
        applied_ops: usize,
        skipped_ops_capacity: usize,
        reuse_failed_missing_window: usize,
        reuse_failed_reconfigure: usize,
        reuse_failed_missing_buffer: usize,
        windows_recovered: usize,
    ) -> Self {
        Self {
            planned_ops,
            applied_ops,
            skipped_ops_capacity,
            reuse_failed_missing_window,
            reuse_failed_reconfigure,
            reuse_failed_missing_buffer,
            windows_recovered,
        }
    }

    pub(crate) const fn planned_ops(self) -> usize {
        self.planned_ops
    }

    pub(crate) const fn applied_ops(self) -> usize {
        self.applied_ops
    }

    pub(crate) const fn skipped_ops_capacity(self) -> usize {
        self.skipped_ops_capacity
    }

    pub(crate) const fn reuse_failed_missing_window(self) -> usize {
        self.reuse_failed_missing_window
    }

    pub(crate) const fn reuse_failed_reconfigure(self) -> usize {
        self.reuse_failed_reconfigure
    }

    pub(crate) const fn reuse_failed_missing_buffer(self) -> usize {
        self.reuse_failed_missing_buffer
    }

    pub(crate) const fn windows_recovered(self) -> usize {
        self.windows_recovered
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum RealizationDivergence {
    ApplyMetrics(DegradedApplyMetrics),
    ShellStateUnknown,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ApplyFailureKind {
    MissingProjection,
    MissingRequiredProbe,
    ShellError,
    ViewportDrift,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) enum RealizationLedger {
    #[default]
    Cleared,
    Consistent {
        acknowledged: ProjectionSnapshot,
    },
    Diverged {
        last_consistent: Option<ProjectionSnapshot>,
        divergence: RealizationDivergence,
    },
}

impl RealizationLedger {
    #[cfg(test)]
    pub(crate) const fn last_consistent(&self) -> Option<&ProjectionSnapshot> {
        match self {
            Self::Cleared => None,
            Self::Consistent { acknowledged } => Some(acknowledged),
            Self::Diverged {
                last_consistent, ..
            } => last_consistent.as_ref(),
        }
    }

    pub(crate) const fn trusted_acknowledged_for_patch(&self) -> Option<&ProjectionSnapshot> {
        match self {
            Self::Cleared | Self::Diverged { .. } => None,
            Self::Consistent { acknowledged } => Some(acknowledged),
        }
    }

    pub(crate) fn cleanup_applied(self) -> Self {
        match self {
            Self::Cleared => Self::Cleared,
            Self::Consistent { acknowledged } => Self::Diverged {
                last_consistent: Some(acknowledged),
                divergence: RealizationDivergence::ShellStateUnknown,
            },
            Self::Diverged {
                last_consistent, ..
            } => Self::Diverged {
                last_consistent,
                divergence: RealizationDivergence::ShellStateUnknown,
            },
        }
    }

    pub(crate) fn acknowledge(target: Option<ProjectionSnapshot>) -> Self {
        target.map_or(Self::Cleared, |acknowledged| Self::Consistent {
            acknowledged,
        })
    }

    pub(crate) fn diverged_from(
        last_consistent: Option<ProjectionSnapshot>,
        divergence: RealizationDivergence,
    ) -> Self {
        Self::Diverged {
            last_consistent,
            divergence,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RealizationDraw {
    palette: PaletteSpec,
    allocation_policy: RenderAllocationPolicy,
    max_kept_windows: usize,
}

impl RealizationDraw {
    pub(crate) fn new(
        palette: PaletteSpec,
        allocation_policy: RenderAllocationPolicy,
        max_kept_windows: usize,
    ) -> Self {
        Self {
            palette,
            allocation_policy,
            max_kept_windows,
        }
    }

    pub(crate) const fn palette(&self) -> &PaletteSpec {
        &self.palette
    }

    pub(crate) const fn allocation_policy(&self) -> RenderAllocationPolicy {
        self.allocation_policy
    }

    pub(crate) const fn max_kept_windows(&self) -> usize {
        self.max_kept_windows
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct RealizationClear {
    max_kept_windows: usize,
}

impl RealizationClear {
    pub(crate) const fn new(max_kept_windows: usize) -> Self {
        Self { max_kept_windows }
    }

    pub(crate) const fn max_kept_windows(self) -> usize {
        self.max_kept_windows
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct RealizationFailure {
    reason: ApplyFailureKind,
    divergence: RealizationDivergence,
}

impl RealizationFailure {
    pub(crate) const fn new(reason: ApplyFailureKind, divergence: RealizationDivergence) -> Self {
        Self { reason, divergence }
    }

    pub(crate) const fn reason(self) -> ApplyFailureKind {
        self.reason
    }

    pub(crate) const fn divergence(self) -> RealizationDivergence {
        self.divergence
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RealizationPlan {
    Draw(RealizationDraw),
    Clear(RealizationClear),
    Noop,
    Failure(RealizationFailure),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum AnimationSchedule {
    Idle,
    DefaultDelay,
    Deadline(Millis),
}

impl AnimationSchedule {
    pub(crate) const fn from_parts(
        should_schedule_next_animation: bool,
        next_animation_at_ms: Option<Millis>,
    ) -> Self {
        match (should_schedule_next_animation, next_animation_at_ms) {
            (false, _) => Self::Idle,
            (true, None) => Self::DefaultDelay,
            (true, Some(deadline)) => Self::Deadline(deadline),
        }
    }

    pub(crate) const fn deadline(self) -> Option<Millis> {
        match self {
            Self::Idle | Self::DefaultDelay => None,
            Self::Deadline(deadline) => Some(deadline),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub(crate) enum ProposalShapeError {
    #[error("draw realization reached a clear patch")]
    DrawReachedClearPatch,
    #[error("draw realization requires a target projection")]
    DrawMissingTargetProjection,
    #[error("clear realization reached a draw patch")]
    ClearReachedDrawPatch,
    #[error("noop realization reached a draw patch")]
    NoopReachedDrawPatch,
    #[error("noop realization reached a clear patch")]
    NoopReachedClearPatch,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ProposalExecution {
    Draw {
        patch: ScenePatch,
        target_projection: ProjectionSnapshot,
        realization: RealizationDraw,
    },
    Clear {
        patch: ScenePatch,
        realization: RealizationClear,
    },
    Noop {
        patch: ScenePatch,
    },
    Failure {
        patch: ScenePatch,
        failure: RealizationFailure,
    },
}

impl ProposalExecution {
    pub(crate) fn draw(
        patch: ScenePatch,
        realization: RealizationDraw,
    ) -> Result<Self, ProposalShapeError> {
        match patch.kind() {
            ScenePatchKind::Clear => Err(ProposalShapeError::DrawReachedClearPatch),
            ScenePatchKind::Noop | ScenePatchKind::Replace => patch
                .basis()
                .target()
                .cloned()
                .map(|target_projection| Self::Draw {
                    patch,
                    target_projection,
                    realization,
                })
                .ok_or(ProposalShapeError::DrawMissingTargetProjection),
        }
    }

    pub(crate) fn clear(
        patch: ScenePatch,
        realization: RealizationClear,
    ) -> Result<Self, ProposalShapeError> {
        match patch.kind() {
            ScenePatchKind::Replace => Err(ProposalShapeError::ClearReachedDrawPatch),
            ScenePatchKind::Noop | ScenePatchKind::Clear => Ok(Self::Clear { patch, realization }),
        }
    }

    pub(crate) fn noop(patch: ScenePatch) -> Result<Self, ProposalShapeError> {
        match patch.kind() {
            ScenePatchKind::Replace => Err(ProposalShapeError::NoopReachedDrawPatch),
            ScenePatchKind::Clear => Err(ProposalShapeError::NoopReachedClearPatch),
            ScenePatchKind::Noop => Ok(Self::Noop { patch }),
        }
    }

    pub(crate) fn failure(patch: ScenePatch, failure: RealizationFailure) -> Self {
        Self::Failure { patch, failure }
    }

    pub(crate) fn patch(&self) -> &ScenePatch {
        match self {
            Self::Draw { patch, .. }
            | Self::Clear { patch, .. }
            | Self::Noop { patch }
            | Self::Failure { patch, .. } => patch,
        }
    }

    pub(crate) fn realization(&self) -> RealizationPlan {
        match self {
            Self::Draw { realization, .. } => RealizationPlan::Draw(realization.clone()),
            Self::Clear { realization, .. } => RealizationPlan::Clear(*realization),
            Self::Noop { .. } => RealizationPlan::Noop,
            Self::Failure { failure, .. } => RealizationPlan::Failure(*failure),
        }
    }

    #[cfg(test)]
    pub(crate) fn draw_realization(&self) -> Option<(&ProjectionSnapshot, &RealizationDraw)> {
        match self {
            Self::Draw {
                target_projection,
                realization,
                ..
            } => Some((target_projection, realization)),
            Self::Clear { .. } | Self::Noop { .. } | Self::Failure { .. } => None,
        }
    }

    pub(crate) fn failure_reason(&self) -> Option<RealizationFailure> {
        match self {
            Self::Failure { failure, .. } => Some(*failure),
            Self::Draw { .. } | Self::Clear { .. } | Self::Noop { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InFlightProposal {
    proposal_id: ProposalId,
    execution: ProposalExecution,
    cleanup_action: RenderCleanupAction,
    side_effects: RenderSideEffects,
    animation_schedule: AnimationSchedule,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedRender {
    next_scene: SceneState,
    proposal: InFlightProposal,
}

impl PlannedRender {
    pub(crate) fn new(next_scene: SceneState, proposal: InFlightProposal) -> Self {
        Self {
            next_scene,
            proposal,
        }
    }

    pub(crate) const fn proposal_id(&self) -> ProposalId {
        self.proposal.proposal_id()
    }

    pub(crate) const fn next_scene(&self) -> &SceneState {
        &self.next_scene
    }

    pub(crate) const fn proposal(&self) -> &InFlightProposal {
        &self.proposal
    }

    pub(crate) fn into_parts(self) -> (SceneState, InFlightProposal) {
        (self.next_scene, self.proposal)
    }
}

impl InFlightProposal {
    fn from_execution(
        proposal_id: ProposalId,
        execution: ProposalExecution,
        cleanup_action: RenderCleanupAction,
        side_effects: RenderSideEffects,
        animation_schedule: AnimationSchedule,
    ) -> Self {
        Self {
            proposal_id,
            execution,
            cleanup_action,
            side_effects,
            animation_schedule,
        }
    }

    pub(crate) fn draw(
        proposal_id: ProposalId,
        patch: ScenePatch,
        realization: RealizationDraw,
        cleanup_action: RenderCleanupAction,
        side_effects: RenderSideEffects,
        animation_schedule: AnimationSchedule,
    ) -> Result<Self, ProposalShapeError> {
        ProposalExecution::draw(patch, realization).map(|execution| {
            Self::from_execution(
                proposal_id,
                execution,
                cleanup_action,
                side_effects,
                animation_schedule,
            )
        })
    }

    pub(crate) fn clear(
        proposal_id: ProposalId,
        patch: ScenePatch,
        realization: RealizationClear,
        cleanup_action: RenderCleanupAction,
        side_effects: RenderSideEffects,
        animation_schedule: AnimationSchedule,
    ) -> Result<Self, ProposalShapeError> {
        ProposalExecution::clear(patch, realization).map(|execution| {
            Self::from_execution(
                proposal_id,
                execution,
                cleanup_action,
                side_effects,
                animation_schedule,
            )
        })
    }

    pub(crate) fn noop(
        proposal_id: ProposalId,
        patch: ScenePatch,
        cleanup_action: RenderCleanupAction,
        side_effects: RenderSideEffects,
        animation_schedule: AnimationSchedule,
    ) -> Result<Self, ProposalShapeError> {
        ProposalExecution::noop(patch).map(|execution| {
            Self::from_execution(
                proposal_id,
                execution,
                cleanup_action,
                side_effects,
                animation_schedule,
            )
        })
    }

    pub(crate) fn failure(
        proposal_id: ProposalId,
        patch: ScenePatch,
        failure: RealizationFailure,
        cleanup_action: RenderCleanupAction,
        side_effects: RenderSideEffects,
        animation_schedule: AnimationSchedule,
    ) -> Self {
        Self::from_execution(
            proposal_id,
            ProposalExecution::failure(patch, failure),
            cleanup_action,
            side_effects,
            animation_schedule,
        )
    }

    pub(crate) const fn proposal_id(&self) -> ProposalId {
        self.proposal_id
    }

    pub(crate) fn basis(&self) -> &PatchBasis {
        self.patch().basis()
    }

    pub(crate) fn patch(&self) -> &ScenePatch {
        self.execution.patch()
    }

    pub(crate) fn realization(&self) -> RealizationPlan {
        self.execution.realization()
    }

    pub(crate) const fn execution(&self) -> &ProposalExecution {
        &self.execution
    }

    pub(crate) fn failure_reason(&self) -> Option<RealizationFailure> {
        self.execution.failure_reason()
    }

    pub(crate) const fn cleanup_action(&self) -> RenderCleanupAction {
        self.cleanup_action
    }

    pub(crate) const fn side_effects(&self) -> RenderSideEffects {
        self.side_effects
    }

    pub(crate) const fn animation_schedule(&self) -> AnimationSchedule {
        self.animation_schedule
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AnimationSchedule, InFlightProposal, ProposalExecution, ProposalShapeError,
        RealizationClear, RealizationDraw, RealizationFailure,
    };
    use crate::core::realization::LogicalRaster;
    use crate::core::runtime_reducer::{
        RenderAllocationPolicy, RenderCleanupAction, RenderSideEffects,
    };
    use crate::core::state::{PatchBasis, ProjectionSnapshot, ProjectionWitness, ScenePatch};
    use crate::core::types::{
        CursorCol, CursorRow, IngressSeq, Millis, ObservationId, ProjectorRevision, ProposalId,
        SceneRevision, ViewportSnapshot,
    };
    use crate::draw::render_plan::CellOp;
    use std::sync::Arc;

    fn projection_snapshot() -> ProjectionSnapshot {
        ProjectionSnapshot::new(
            ProjectionWitness::new(
                SceneRevision::INITIAL,
                ObservationId::from_ingress_seq(IngressSeq::new(1)),
                ViewportSnapshot::new(CursorRow(20), CursorCol(40)),
                ProjectorRevision::CURRENT,
            ),
            LogicalRaster::new(None, Arc::from(Vec::<CellOp>::new())),
        )
    }

    fn draw_plan() -> RealizationDraw {
        RealizationDraw::new(
            crate::core::realization::PaletteSpec::from_frame(&crate::types::RenderFrame {
                mode: "n".to_string(),
                corners: [crate::types::Point { row: 1.0, col: 1.0 }; 4],
                step_samples: Vec::new().into(),
                planner_idle_steps: 0,
                target: crate::types::Point { row: 1.0, col: 1.0 },
                target_corners: [crate::types::Point { row: 1.0, col: 1.0 }; 4],
                vertical_bar: false,
                trail_stroke_id: crate::core::types::StrokeId::new(1),
                retarget_epoch: 1,
                particles: Vec::new().into(),
                color_at_cursor: None,
                static_config: Arc::new(crate::types::StaticRenderConfig {
                    cursor_color: None,
                    cursor_color_insert_mode: None,
                    normal_bg: None,
                    transparent_bg_fallback_color: String::new(),
                    cterm_cursor_colors: None,
                    cterm_bg: None,
                    hide_target_hack: false,
                    max_kept_windows: 32,
                    never_draw_over_target: false,
                    particle_max_lifetime: 0.0,
                    particle_switch_octant_braille: 0.0,
                    particles_over_text: true,
                    color_levels: 16,
                    gamma: 1.0,
                    block_aspect_ratio: crate::config::DEFAULT_BLOCK_ASPECT_RATIO,
                    tail_duration_ms: 0.0,
                    simulation_hz: 0.0,
                    trail_thickness: 0.0,
                    trail_thickness_x: 0.0,
                    spatial_coherence_weight: 0.0,
                    temporal_stability_weight: 0.0,
                    top_k_per_cell: 1,
                    windows_zindex: 1,
                }),
            }),
            RenderAllocationPolicy::ReuseOnly,
            32,
        )
    }

    #[test]
    fn draw_proposal_rejects_clear_patch() {
        let patch = ScenePatch::derive(PatchBasis::new(Some(projection_snapshot()), None));

        let result = ProposalExecution::draw(patch, draw_plan());

        assert_eq!(result, Err(ProposalShapeError::DrawReachedClearPatch));
    }

    #[test]
    fn draw_proposal_keeps_target_projection_when_patch_kind_is_noop() {
        let projection = projection_snapshot();
        let patch = ScenePatch::derive(PatchBasis::new(
            Some(projection.clone()),
            Some(projection.clone()),
        ));

        let proposal = InFlightProposal::draw(
            ProposalId::new(1),
            patch,
            draw_plan(),
            RenderCleanupAction::NoAction,
            RenderSideEffects::default(),
            AnimationSchedule::Idle,
        )
        .expect("draw proposal with noop patch should keep the target projection");

        let Some((target_projection, _)) = proposal.execution().draw_realization() else {
            panic!("expected draw proposal execution");
        };
        assert_eq!(target_projection, &projection);
    }

    #[test]
    fn clear_proposal_rejects_draw_patch() {
        let patch = ScenePatch::derive(PatchBasis::new(None, Some(projection_snapshot())));

        let result = InFlightProposal::clear(
            ProposalId::new(1),
            patch,
            RealizationClear::new(12),
            RenderCleanupAction::NoAction,
            RenderSideEffects::default(),
            AnimationSchedule::Idle,
        );

        assert_eq!(result, Err(ProposalShapeError::ClearReachedDrawPatch));
    }

    #[test]
    fn noop_proposal_rejects_clear_patch() {
        let patch = ScenePatch::derive(PatchBasis::new(Some(projection_snapshot()), None));

        let result = InFlightProposal::noop(
            ProposalId::new(1),
            patch,
            RenderCleanupAction::NoAction,
            RenderSideEffects::default(),
            AnimationSchedule::Idle,
        );

        assert_eq!(result, Err(ProposalShapeError::NoopReachedClearPatch));
    }

    #[test]
    fn failure_proposal_preserves_failure_without_shell_shape_validation() {
        let failure = RealizationFailure::new(
            crate::core::state::ApplyFailureKind::MissingProjection,
            crate::core::state::RealizationDivergence::ShellStateUnknown,
        );
        let proposal = InFlightProposal::failure(
            ProposalId::new(1),
            ScenePatch::derive(PatchBasis::new(None, None)),
            failure,
            RenderCleanupAction::NoAction,
            RenderSideEffects::default(),
            AnimationSchedule::Idle,
        );

        assert_eq!(proposal.failure_reason(), Some(failure));
    }

    #[test]
    fn acknowledge_without_target_keeps_the_ledger_cleared() {
        assert_eq!(
            crate::core::state::RealizationLedger::acknowledge(None),
            crate::core::state::RealizationLedger::Cleared
        );
    }

    #[test]
    fn cleanup_applied_from_consistent_moves_the_acknowledged_snapshot_into_divergence() {
        let acknowledged = projection_snapshot();

        let cleaned = crate::core::state::RealizationLedger::Consistent {
            acknowledged: acknowledged.clone(),
        }
        .cleanup_applied();

        assert_eq!(
            cleaned,
            crate::core::state::RealizationLedger::Diverged {
                last_consistent: Some(acknowledged),
                divergence: crate::core::state::RealizationDivergence::ShellStateUnknown,
            }
        );
    }

    #[test]
    fn animation_schedule_from_parts_preserves_idle_default_and_deadline_states() {
        assert_eq!(
            AnimationSchedule::from_parts(false, Some(Millis::new(9))),
            AnimationSchedule::Idle
        );
        assert_eq!(
            AnimationSchedule::from_parts(true, None),
            AnimationSchedule::DefaultDelay
        );
        assert_eq!(
            AnimationSchedule::from_parts(true, Some(Millis::new(11))),
            AnimationSchedule::Deadline(Millis::new(11))
        );
    }
}

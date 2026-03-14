use super::{PatchBasis, ProjectionSnapshot, ScenePatch, SceneState};
use crate::core::realization::PaletteSpec;
use crate::core::runtime_reducer::{
    RenderAllocationPolicy, RenderCleanupAction, RenderSideEffects,
};
use crate::core::types::{Millis, ProposalId};

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
        acknowledged: Option<ProjectionSnapshot>,
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
            Self::Consistent { acknowledged } => acknowledged.as_ref(),
            Self::Diverged {
                last_consistent, ..
            } => last_consistent.as_ref(),
        }
    }

    pub(crate) const fn trusted_acknowledged_for_patch(&self) -> Option<&ProjectionSnapshot> {
        match self {
            Self::Cleared | Self::Diverged { .. } => None,
            Self::Consistent { acknowledged } => acknowledged.as_ref(),
        }
    }

    pub(crate) fn cleanup_applied(self) -> Self {
        match self {
            Self::Cleared => Self::Cleared,
            Self::Consistent { acknowledged } => {
                if acknowledged.is_some() {
                    Self::Diverged {
                        last_consistent: acknowledged,
                        divergence: RealizationDivergence::ShellStateUnknown,
                    }
                } else {
                    Self::Cleared
                }
            }
            Self::Diverged {
                last_consistent, ..
            } => Self::Diverged {
                last_consistent,
                divergence: RealizationDivergence::ShellStateUnknown,
            },
        }
    }

    pub(crate) fn acknowledge(target: Option<ProjectionSnapshot>) -> Self {
        match target {
            Some(snapshot) => Self::Consistent {
                acknowledged: Some(snapshot),
            },
            None => Self::Cleared,
        }
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InFlightProposal {
    proposal_id: ProposalId,
    patch: ScenePatch,
    realization: RealizationPlan,
    cleanup_action: RenderCleanupAction,
    side_effects: RenderSideEffects,
    should_schedule_next_animation: bool,
    next_animation_at_ms: Option<Millis>,
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
    pub(crate) fn new(
        proposal_id: ProposalId,
        patch: ScenePatch,
        realization: RealizationPlan,
        cleanup_action: RenderCleanupAction,
        side_effects: RenderSideEffects,
        should_schedule_next_animation: bool,
        next_animation_at_ms: Option<Millis>,
    ) -> Self {
        Self {
            proposal_id,
            patch,
            realization,
            cleanup_action,
            side_effects,
            should_schedule_next_animation,
            next_animation_at_ms,
        }
    }

    pub(crate) const fn proposal_id(&self) -> ProposalId {
        self.proposal_id
    }

    pub(crate) const fn basis(&self) -> &PatchBasis {
        self.patch.basis()
    }

    pub(crate) const fn patch(&self) -> &ScenePatch {
        &self.patch
    }

    pub(crate) const fn realization(&self) -> &RealizationPlan {
        &self.realization
    }

    pub(crate) const fn cleanup_action(&self) -> RenderCleanupAction {
        self.cleanup_action
    }

    pub(crate) const fn side_effects(&self) -> RenderSideEffects {
        self.side_effects
    }

    pub(crate) const fn should_schedule_next_animation(&self) -> bool {
        self.should_schedule_next_animation
    }

    pub(crate) const fn next_animation_at_ms(&self) -> Option<Millis> {
        self.next_animation_at_ms
    }
}

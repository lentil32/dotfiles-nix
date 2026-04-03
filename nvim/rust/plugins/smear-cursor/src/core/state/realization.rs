use super::PatchBasis;
use super::ProjectionSnapshot;
use super::ScenePatch;
use super::ScenePatchKind;
use super::SceneState;
use crate::core::realization::PaletteSpec;
use crate::core::runtime_reducer::RenderAllocationPolicy;
use crate::core::runtime_reducer::RenderCleanupAction;
use crate::core::runtime_reducer::RenderSideEffects;
use crate::core::types::Millis;
use crate::core::types::ProposalId;
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
    use super::AnimationSchedule;
    use super::DegradedApplyMetrics;
    use super::InFlightProposal;
    use super::ProposalShapeError;
    use super::RealizationClear;
    use super::RealizationDivergence;
    use super::RealizationDraw;
    use super::RealizationFailure;
    use super::RealizationLedger;
    use super::RealizationPlan;
    use crate::core::realization::LogicalRaster;
    use crate::core::runtime_reducer::CursorVisibilityEffect;
    use crate::core::runtime_reducer::RenderAllocationPolicy;
    use crate::core::runtime_reducer::RenderCleanupAction;
    use crate::core::runtime_reducer::RenderSideEffects;
    use crate::core::runtime_reducer::TargetCellPresentation;
    use crate::core::state::PatchBasis;
    use crate::core::state::ProjectionSnapshot;
    use crate::core::state::ProjectionWitness;
    use crate::core::state::ScenePatch;
    use crate::core::state::ScenePatchKind;
    use crate::core::types::CursorCol;
    use crate::core::types::CursorRow;
    use crate::core::types::IngressSeq;
    use crate::core::types::Millis;
    use crate::core::types::ObservationId;
    use crate::core::types::ProjectorRevision;
    use crate::core::types::ProposalId;
    use crate::core::types::SceneRevision;
    use crate::core::types::ViewportSnapshot;
    use crate::draw::render_plan::CellOp;
    use crate::test_support::proptest::pure_config;
    use crate::types::CursorCellShape;
    use proptest::prelude::*;
    use std::sync::Arc;

    #[derive(Clone, Debug)]
    struct PatchFixture {
        patch: ScenePatch,
        expected_target: Option<ProjectionSnapshot>,
    }

    fn projection_snapshot_with_ingress_seq(ingress_seq: u64) -> ProjectionSnapshot {
        ProjectionSnapshot::new(
            ProjectionWitness::new(
                SceneRevision::INITIAL,
                ObservationId::from_ingress_seq(IngressSeq::new(ingress_seq)),
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

    fn render_cleanup_action_strategy() -> BoxedStrategy<RenderCleanupAction> {
        prop_oneof![
            Just(RenderCleanupAction::NoAction),
            Just(RenderCleanupAction::Schedule),
            Just(RenderCleanupAction::Invalidate),
        ]
        .boxed()
    }

    fn target_cell_presentation_strategy() -> BoxedStrategy<TargetCellPresentation> {
        prop_oneof![
            Just(TargetCellPresentation::None),
            Just(TargetCellPresentation::OverlayCursorCell(
                CursorCellShape::Block
            )),
            Just(TargetCellPresentation::OverlayCursorCell(
                CursorCellShape::VerticalBar,
            )),
            Just(TargetCellPresentation::OverlayCursorCell(
                CursorCellShape::HorizontalBar,
            )),
        ]
        .boxed()
    }

    fn cursor_visibility_effect_strategy() -> BoxedStrategy<CursorVisibilityEffect> {
        prop_oneof![
            Just(CursorVisibilityEffect::Keep),
            Just(CursorVisibilityEffect::Hide),
            Just(CursorVisibilityEffect::Show),
        ]
        .boxed()
    }

    fn render_side_effects_strategy() -> BoxedStrategy<RenderSideEffects> {
        (
            any::<bool>(),
            any::<bool>(),
            target_cell_presentation_strategy(),
            cursor_visibility_effect_strategy(),
            any::<bool>(),
        )
            .prop_map(
                |(
                    redraw_after_draw_if_cmdline,
                    redraw_after_clear_if_cmdline,
                    target_cell_presentation,
                    cursor_visibility,
                    allow_real_cursor_updates,
                )| RenderSideEffects {
                    redraw_after_draw_if_cmdline,
                    redraw_after_clear_if_cmdline,
                    target_cell_presentation,
                    cursor_visibility,
                    allow_real_cursor_updates,
                },
            )
            .boxed()
    }

    fn animation_schedule_strategy() -> BoxedStrategy<AnimationSchedule> {
        (any::<bool>(), proptest::option::of(any::<u64>()))
            .prop_map(|(should_schedule_next_animation, next_animation_at_ms)| {
                AnimationSchedule::from_parts(
                    should_schedule_next_animation,
                    next_animation_at_ms.map(Millis::new),
                )
            })
            .boxed()
    }

    fn proposal_patch_fixture_strategy() -> BoxedStrategy<PatchFixture> {
        prop_oneof![
            Just(PatchFixture {
                patch: ScenePatch::derive(PatchBasis::new(None, None)),
                expected_target: None,
            }),
            (1_u64..=u16::MAX as u64).prop_map(|seq| {
                let target = projection_snapshot_with_ingress_seq(seq);
                PatchFixture {
                    patch: ScenePatch::derive(PatchBasis::new(
                        Some(target.clone()),
                        Some(target.clone()),
                    )),
                    expected_target: Some(target),
                }
            }),
            (1_u64..=u16::MAX as u64).prop_map(|seq| PatchFixture {
                patch: ScenePatch::derive(PatchBasis::new(
                    Some(projection_snapshot_with_ingress_seq(seq)),
                    None,
                )),
                expected_target: None,
            }),
            (1_u64..=u16::MAX as u64).prop_map(|seq| {
                let target = projection_snapshot_with_ingress_seq(seq);
                PatchFixture {
                    patch: ScenePatch::derive(PatchBasis::new(None, Some(target.clone()))),
                    expected_target: Some(target),
                }
            }),
            (
                (1_u64..=u16::MAX as u64),
                (u16::MAX as u64 + 1)..=(u16::MAX as u64 * 2)
            )
                .prop_map(|(current_seq, target_seq)| {
                    let target = projection_snapshot_with_ingress_seq(target_seq);
                    PatchFixture {
                        patch: ScenePatch::derive(PatchBasis::new(
                            Some(projection_snapshot_with_ingress_seq(current_seq)),
                            Some(target.clone()),
                        )),
                        expected_target: Some(target),
                    }
                }),
        ]
        .boxed()
    }

    fn realization_divergence_strategy() -> BoxedStrategy<RealizationDivergence> {
        prop_oneof![
            Just(RealizationDivergence::ShellStateUnknown),
            (
                0_usize..=64_usize,
                0_usize..=64_usize,
                0_usize..=64_usize,
                0_usize..=64_usize,
                0_usize..=64_usize,
                0_usize..=64_usize,
                0_usize..=64_usize,
            )
                .prop_map(
                    |(
                        planned_ops,
                        applied_ops,
                        skipped_ops_capacity,
                        reuse_failed_missing_window,
                        reuse_failed_reconfigure,
                        reuse_failed_missing_buffer,
                        windows_recovered,
                    )| {
                        RealizationDivergence::ApplyMetrics(DegradedApplyMetrics::new(
                            planned_ops,
                            applied_ops,
                            skipped_ops_capacity,
                            reuse_failed_missing_window,
                            reuse_failed_reconfigure,
                            reuse_failed_missing_buffer,
                            windows_recovered,
                        ))
                    },
                ),
        ]
        .boxed()
    }

    fn realization_ledger_strategy() -> BoxedStrategy<RealizationLedger> {
        prop_oneof![
            Just(RealizationLedger::Cleared),
            (1_u64..=u16::MAX as u64).prop_map(|seq| RealizationLedger::Consistent {
                acknowledged: projection_snapshot_with_ingress_seq(seq),
            }),
            (
                proptest::option::of(1_u64..=u16::MAX as u64),
                realization_divergence_strategy(),
            )
                .prop_map(|(last_consistent_seq, divergence)| {
                    RealizationLedger::Diverged {
                        last_consistent: last_consistent_seq
                            .map(projection_snapshot_with_ingress_seq),
                        divergence,
                    }
                }),
        ]
        .boxed()
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_proposal_constructors_accept_only_compatible_patch_shapes(
            patch_fixture in proposal_patch_fixture_strategy(),
            proposal_id in any::<u64>().prop_map(ProposalId::new),
            cleanup_action in render_cleanup_action_strategy(),
            side_effects in render_side_effects_strategy(),
            animation_schedule in animation_schedule_strategy(),
            max_kept_windows in 0_usize..=64_usize,
        ) {
            let draw_realization = draw_plan();
            let clear_realization = RealizationClear::new(max_kept_windows);

            let draw_result = InFlightProposal::draw(
                proposal_id,
                patch_fixture.patch.clone(),
                draw_realization.clone(),
                cleanup_action,
                side_effects,
                animation_schedule,
            );

            match (patch_fixture.patch.kind(), patch_fixture.expected_target.as_ref()) {
                (ScenePatchKind::Clear, _) => {
                    prop_assert_eq!(draw_result, Err(ProposalShapeError::DrawReachedClearPatch));
                }
                (ScenePatchKind::Noop | ScenePatchKind::Replace, None) => {
                    prop_assert_eq!(draw_result, Err(ProposalShapeError::DrawMissingTargetProjection));
                }
                (ScenePatchKind::Noop | ScenePatchKind::Replace, Some(expected_target)) => {
                    let proposal = draw_result.expect("draw-compatible patch fixture should succeed");
                    prop_assert_eq!(proposal.proposal_id(), proposal_id);
                    prop_assert_eq!(proposal.patch(), &patch_fixture.patch);
                    prop_assert_eq!(proposal.cleanup_action(), cleanup_action);
                    prop_assert_eq!(proposal.side_effects(), side_effects);
                    prop_assert_eq!(proposal.animation_schedule(), animation_schedule);
                    prop_assert_eq!(
                        proposal.realization(),
                        RealizationPlan::Draw(draw_realization.clone())
                    );
                    prop_assert_eq!(proposal.failure_reason(), None);

                    let Some((target_projection, realization)) =
                        proposal.execution().draw_realization()
                    else {
                        prop_assert!(false, "draw proposal should expose draw realization");
                        return Ok(());
                    };
                    prop_assert_eq!(target_projection, expected_target);
                    prop_assert_eq!(realization, &draw_realization);
                }
            }

            let clear_result = InFlightProposal::clear(
                proposal_id,
                patch_fixture.patch.clone(),
                clear_realization,
                cleanup_action,
                side_effects,
                animation_schedule,
            );

            match patch_fixture.patch.kind() {
                ScenePatchKind::Replace => {
                    prop_assert_eq!(clear_result, Err(ProposalShapeError::ClearReachedDrawPatch));
                }
                ScenePatchKind::Noop | ScenePatchKind::Clear => {
                    let proposal = clear_result.expect("clear-compatible patch fixture should succeed");
                    prop_assert_eq!(proposal.proposal_id(), proposal_id);
                    prop_assert_eq!(proposal.patch(), &patch_fixture.patch);
                    prop_assert_eq!(proposal.cleanup_action(), cleanup_action);
                    prop_assert_eq!(proposal.side_effects(), side_effects);
                    prop_assert_eq!(proposal.animation_schedule(), animation_schedule);
                    prop_assert_eq!(proposal.realization(), RealizationPlan::Clear(clear_realization));
                    prop_assert_eq!(proposal.failure_reason(), None);
                }
            }

            let noop_result = InFlightProposal::noop(
                proposal_id,
                patch_fixture.patch.clone(),
                cleanup_action,
                side_effects,
                animation_schedule,
            );

            match patch_fixture.patch.kind() {
                ScenePatchKind::Replace => {
                    prop_assert_eq!(noop_result, Err(ProposalShapeError::NoopReachedDrawPatch));
                }
                ScenePatchKind::Clear => {
                    prop_assert_eq!(noop_result, Err(ProposalShapeError::NoopReachedClearPatch));
                }
                ScenePatchKind::Noop => {
                    let proposal = noop_result.expect("noop patch fixture should succeed");
                    prop_assert_eq!(proposal.proposal_id(), proposal_id);
                    prop_assert_eq!(proposal.patch(), &patch_fixture.patch);
                    prop_assert_eq!(proposal.cleanup_action(), cleanup_action);
                    prop_assert_eq!(proposal.side_effects(), side_effects);
                    prop_assert_eq!(proposal.animation_schedule(), animation_schedule);
                    prop_assert_eq!(proposal.realization(), RealizationPlan::Noop);
                    prop_assert_eq!(proposal.failure_reason(), None);
                }
            }
        }

        #[test]
        fn prop_failure_proposal_preserves_failure_and_metadata_for_any_patch_shape(
            patch_fixture in proposal_patch_fixture_strategy(),
            proposal_id in any::<u64>().prop_map(ProposalId::new),
            cleanup_action in render_cleanup_action_strategy(),
            side_effects in render_side_effects_strategy(),
            animation_schedule in animation_schedule_strategy(),
            failure_reason in prop_oneof![
                Just(crate::core::state::ApplyFailureKind::MissingProjection),
                Just(crate::core::state::ApplyFailureKind::MissingRequiredProbe),
                Just(crate::core::state::ApplyFailureKind::ShellError),
                Just(crate::core::state::ApplyFailureKind::ViewportDrift),
            ],
            divergence in realization_divergence_strategy(),
        ) {
            let failure = RealizationFailure::new(failure_reason, divergence);
            let proposal = InFlightProposal::failure(
                proposal_id,
                patch_fixture.patch.clone(),
                failure,
                cleanup_action,
                side_effects,
                animation_schedule,
            );

            prop_assert_eq!(proposal.proposal_id(), proposal_id);
            prop_assert_eq!(proposal.patch(), &patch_fixture.patch);
            prop_assert_eq!(proposal.cleanup_action(), cleanup_action);
            prop_assert_eq!(proposal.side_effects(), side_effects);
            prop_assert_eq!(proposal.animation_schedule(), animation_schedule);
            prop_assert_eq!(proposal.realization(), RealizationPlan::Failure(failure));
            prop_assert_eq!(proposal.failure_reason(), Some(failure));
        }

        #[test]
        fn prop_realization_ledger_constructors_preserve_target_and_cleanup_invariants(
            acknowledged_seq in proptest::option::of(1_u64..=u16::MAX as u64),
            ledger in realization_ledger_strategy(),
        ) {
            let acknowledged = acknowledged_seq.map(projection_snapshot_with_ingress_seq);

            prop_assert_eq!(
                RealizationLedger::acknowledge(acknowledged.clone()),
                match acknowledged {
                    Some(acknowledged) => RealizationLedger::Consistent { acknowledged },
                    None => RealizationLedger::Cleared,
                }
            );

            let expected_last_consistent = ledger.last_consistent().cloned();
            let was_cleared = matches!(ledger, RealizationLedger::Cleared);
            let cleaned = ledger.cleanup_applied();

            if was_cleared {
                prop_assert_eq!(cleaned, RealizationLedger::Cleared);
            } else {
                prop_assert_eq!(
                    cleaned,
                    RealizationLedger::Diverged {
                        last_consistent: expected_last_consistent,
                        divergence: RealizationDivergence::ShellStateUnknown,
                    }
                );
            }
        }

        #[test]
        fn prop_animation_schedule_from_parts_preserves_idle_default_and_deadline_states(
            should_schedule_next_animation in any::<bool>(),
            next_animation_at_ms in proptest::option::of(any::<u64>()),
        ) {
            let next_animation_at_ms = next_animation_at_ms.map(Millis::new);
            let schedule = AnimationSchedule::from_parts(
                should_schedule_next_animation,
                next_animation_at_ms,
            );

            prop_assert_eq!(
                schedule,
                match (should_schedule_next_animation, next_animation_at_ms) {
                    (false, _) => AnimationSchedule::Idle,
                    (true, None) => AnimationSchedule::DefaultDelay,
                    (true, Some(deadline)) => AnimationSchedule::Deadline(deadline),
                }
            );
            prop_assert_eq!(schedule.deadline(), next_animation_at_ms.filter(|_| matches!(schedule, AnimationSchedule::Deadline(_))));
        }
    }
}

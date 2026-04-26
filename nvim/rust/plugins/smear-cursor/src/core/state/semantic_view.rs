use super::CoreState;
use super::CursorTrailSemantic;
use super::DemandQueue;
use super::InFlightProposal;
use super::ObservationSnapshot;
use super::PatchBasis;
use super::PendingObservation;
use super::ProjectionHandle;
use super::ProtocolPhaseKind;
use super::RealizationDivergence;
use super::RealizationLedger;
use super::RealizationPlan;
use crate::core::runtime_reducer::RenderCleanupAction;
use crate::core::runtime_reducer::RenderSideEffects;
use crate::core::state::AnimationSchedule;
use crate::core::state::ProjectionSemanticView;
use crate::core::types::Generation;
use crate::core::types::MotionRevision;
use crate::core::types::ProposalId;
use crate::core::types::SemanticRevision;
use crate::position::ScreenCell;
use crate::state::RuntimeSemanticView;

// Cache-free projection of projection-owned reducer state. Equality intentionally
// ignores retained projection reuse caches and cached shell materialization.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct ProjectionStateSemanticView {
    motion_revision: MotionRevision,
    last_motion_fingerprint: Option<u64>,
}

// Cache-free projection of reducer-owned scene semantics. Equality ignores
// retained projection caches while preserving authoritative semantic identity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SceneSemanticView<'a> {
    semantic_revision: SemanticRevision,
    cursor_trail: Option<&'a CursorTrailSemantic>,
    projection: ProjectionStateSemanticView,
}

// Cache-free patch basis view. Equality compares authoritative projection
// witnesses and logical rasters rather than projection reuse-key cache state.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct PatchBasisSemanticView<'a> {
    acknowledged: Option<ProjectionSemanticView<'a>>,
    target: Option<ProjectionSemanticView<'a>>,
    kind: super::ScenePatchKind,
}

// Cache-free proposal view. Equality ignores cache-only drift inside retained
// projections while preserving the authoritative proposal payload.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InFlightProposalSemanticView<'a> {
    proposal_id: ProposalId,
    basis: PatchBasisSemanticView<'a>,
    realization: RealizationPlan,
    cleanup_action: RenderCleanupAction,
    side_effects: RenderSideEffects,
    animation_schedule: AnimationSchedule,
}

// Cache-free realization-ledger view. Equality tracks shell-trusted projection
// identity without comparing cached projection materialization internals.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum RealizationLedgerSemanticView<'a> {
    Cleared,
    Consistent {
        acknowledged: ProjectionSemanticView<'a>,
    },
    Diverged {
        last_consistent: Option<ProjectionSemanticView<'a>>,
        divergence: RealizationDivergence,
    },
}

// Cache-free projection of reducer-owned core state. Equality intentionally
// ignores runtime scratch buffers, projection reuse caches, and cached shell
// materialization while preserving authoritative protocol, scene, and
// realization state.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CoreStateSemanticView<'a> {
    generation: Generation,
    phase_kind: ProtocolPhaseKind,
    demand_queue: &'a DemandQueue,
    timers: super::TimerState,
    recovery_policy: super::RecoveryPolicyState,
    ingress_policy: super::IngressPolicyState,
    render_cleanup: super::RenderCleanupState,
    pending_observation: Option<&'a PendingObservation>,
    phase_observation: Option<&'a ObservationSnapshot>,
    probe_refresh_state: Option<super::ProbeRefreshState>,
    pending_plan_proposal_id: Option<ProposalId>,
    pending_proposal: Option<InFlightProposalSemanticView<'a>>,
    entropy: super::EntropyState,
    latest_exact_cursor_cell: Option<ScreenCell>,
    runtime: RuntimeSemanticView<'a>,
    scene: SceneSemanticView<'a>,
    realization: RealizationLedgerSemanticView<'a>,
}

impl PatchBasis {
    pub(crate) fn semantic_view(&self) -> PatchBasisSemanticView<'_> {
        PatchBasisSemanticView {
            acknowledged: self
                .acknowledged_handle()
                .map(ProjectionHandle::semantic_view),
            target: self.target_handle().map(ProjectionHandle::semantic_view),
            kind: self.kind(),
        }
    }
}

impl InFlightProposal {
    pub(crate) fn semantic_view(&self) -> InFlightProposalSemanticView<'_> {
        InFlightProposalSemanticView {
            proposal_id: self.proposal_id(),
            basis: self.basis().semantic_view(),
            realization: self.realization(),
            cleanup_action: self.cleanup_action(),
            side_effects: self.side_effects(),
            animation_schedule: self.animation_schedule(),
        }
    }
}

impl RealizationLedger {
    pub(crate) fn semantic_view(&self) -> RealizationLedgerSemanticView<'_> {
        match self {
            Self::Cleared => RealizationLedgerSemanticView::Cleared,
            Self::Consistent { acknowledged } => RealizationLedgerSemanticView::Consistent {
                acknowledged: acknowledged.semantic_view(),
            },
            Self::Diverged {
                last_consistent,
                divergence,
            } => RealizationLedgerSemanticView::Diverged {
                last_consistent: last_consistent
                    .as_ref()
                    .map(ProjectionHandle::semantic_view),
                divergence: *divergence,
            },
        }
    }
}

impl CoreState {
    pub(crate) fn semantic_view(&self) -> CoreStateSemanticView<'_> {
        CoreStateSemanticView {
            generation: self.generation(),
            phase_kind: self.phase_kind(),
            demand_queue: self.demand_queue(),
            timers: self.timers(),
            recovery_policy: self.recovery_policy(),
            ingress_policy: self.ingress_policy(),
            render_cleanup: self.render_cleanup(),
            pending_observation: self.pending_observation(),
            phase_observation: self.phase_observation(),
            probe_refresh_state: self.probe_refresh_state(),
            pending_plan_proposal_id: self.pending_plan_proposal_id(),
            pending_proposal: self.pending_proposal().map(InFlightProposal::semantic_view),
            entropy: self.entropy(),
            latest_exact_cursor_cell: self.latest_exact_cursor_cell(),
            runtime: self.runtime().semantic_view(),
            scene: SceneSemanticView {
                semantic_revision: self.semantic_state().revision(),
                cursor_trail: self.semantic_state().cursor_trail(),
                projection: ProjectionStateSemanticView {
                    motion_revision: self.projection_state().motion_revision(),
                    last_motion_fingerprint: self.projection_state().last_motion_fingerprint(),
                },
            },
            realization: self.realization().semantic_view(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CoreState;
    use super::InFlightProposal;
    use super::RealizationLedger;
    use crate::core::realization::LogicalRaster;
    use crate::core::runtime_reducer::CursorVisibilityEffect;
    use crate::core::runtime_reducer::MotionClass;
    use crate::core::runtime_reducer::RenderAction;
    use crate::core::runtime_reducer::RenderAllocationPolicy;
    use crate::core::runtime_reducer::RenderCleanupAction;
    use crate::core::runtime_reducer::RenderDecision;
    use crate::core::runtime_reducer::RenderSideEffects;
    use crate::core::runtime_reducer::TargetCellPresentation;
    use crate::core::state::BufferPerfClass;
    use crate::core::state::ExternalDemand;
    use crate::core::state::ExternalDemandKind;
    use crate::core::state::ObservationBasis;
    use crate::core::state::ObservationMotion;
    use crate::core::state::ObservationSnapshot;
    use crate::core::state::PatchBasis;
    use crate::core::state::PendingObservation;
    use crate::core::state::PreparedObservationPlan;
    use crate::core::state::ProbeRequestSet;
    use crate::core::state::ProjectionReuseKey;
    use crate::core::state::ProjectionWitness;
    use crate::core::state::RetainedProjection;
    use crate::core::state::ScenePatch;
    use crate::core::types::IngressSeq;
    use crate::core::types::Millis;
    use crate::core::types::ProjectionPolicyRevision;
    use crate::core::types::ProjectorRevision;
    use crate::core::types::ProposalId;
    use crate::core::types::RenderRevision;
    use crate::position::BufferLine;
    use crate::position::CursorObservation;
    use crate::position::ObservedCell;
    use crate::position::RenderPoint;
    use crate::position::ScreenCell;
    use crate::position::SurfaceId;
    use crate::position::ViewportBounds;
    use crate::position::WindowSurfaceSnapshot;
    use crate::state::CursorShape;
    use crate::state::RuntimeState;
    use crate::state::TrackedCursor;
    use crate::types::Particle;
    use crate::types::StepOutput;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    fn retained_projection(trail_signature: Option<u64>) -> crate::core::state::ProjectionHandle {
        RetainedProjection::new(
            ProjectionWitness::new(
                RenderRevision::INITIAL,
                crate::core::types::ObservationId::from_ingress_seq(IngressSeq::new(7)),
                ViewportBounds::new(20, 40).expect("positive viewport bounds"),
                ProjectorRevision::CURRENT,
            ),
            ProjectionReuseKey::new(
                trail_signature,
                None,
                None,
                TargetCellPresentation::None,
                ProjectionPolicyRevision::INITIAL,
            ),
            crate::draw::render_plan::PlannerState::default(),
            LogicalRaster::new(None, Arc::default()),
        )
        .into_handle()
    }

    fn particle_runtime() -> RuntimeState {
        let mut runtime = RuntimeState::default();
        runtime.config.particles_over_text = false;
        let tracked_cursor = TrackedCursor::fixture(10, 20, 1, 1);
        let shape = CursorShape::block();
        let position = RenderPoint { row: 3.0, col: 4.0 };
        runtime.initialize_cursor(position, shape, 7, &tracked_cursor);
        runtime.apply_step_output(StepOutput {
            current_corners: runtime.current_corners(),
            velocity_corners: runtime.velocity_corners(),
            spring_velocity_corners: runtime.spring_velocity_corners(),
            trail_elapsed_ms: runtime.trail_elapsed_ms(),
            particles: vec![Particle {
                position,
                velocity: RenderPoint::ZERO,
                lifetime: 1.0,
            }],
            previous_center: runtime.previous_center(),
            index_head: 0,
            index_tail: 0,
            rng_state: runtime.rng_state(),
        });
        runtime
    }

    fn observation_request(seq: u64) -> PendingObservation {
        PendingObservation::new(
            ExternalDemand::new(
                IngressSeq::new(seq),
                ExternalDemandKind::ExternalCursor,
                Millis::new(10),
                BufferPerfClass::Full,
            ),
            ProbeRequestSet::default(),
        )
    }

    fn observation_snapshot(seq: u64) -> ObservationSnapshot {
        let request = observation_request(seq);
        ObservationSnapshot::new(
            request,
            ObservationBasis::new(
                Millis::new(11),
                "n".to_string(),
                WindowSurfaceSnapshot::new(
                    SurfaceId::new(1, 1).expect("positive handles"),
                    BufferLine::new(1).expect("positive top buffer line"),
                    0,
                    0,
                    ScreenCell::new(1, 1).expect("one-based window origin"),
                    ViewportBounds::new(20, 40).expect("positive window size"),
                ),
                CursorObservation::new(
                    BufferLine::new(1).expect("positive buffer line"),
                    ObservedCell::Exact(ScreenCell::new(4, 5).expect("positive cursor position")),
                ),
                ViewportBounds::new(20, 40).expect("positive viewport bounds"),
            ),
            ObservationMotion::default(),
        )
    }

    fn prepared_observation_plan() -> PreparedObservationPlan {
        PreparedObservationPlan::new(
            RuntimeState::default().prepared_motion(),
            crate::core::runtime_reducer::CursorTransition {
                render_decision: RenderDecision {
                    render_action: RenderAction::Noop,
                    render_cleanup_action: RenderCleanupAction::NoAction,
                    render_allocation_policy: RenderAllocationPolicy::ReuseOnly,
                    render_side_effects: RenderSideEffects::default(),
                },
                motion_class: MotionClass::Continuous,
                animation_schedule: crate::core::state::AnimationSchedule::Idle,
            },
        )
    }

    #[test]
    fn patch_basis_semantic_view_ignores_projection_reuse_key_drift() {
        let first = PatchBasis::new(
            Some(retained_projection(Some(11))),
            Some(retained_projection(Some(17))),
        );
        let second = PatchBasis::new(
            Some(retained_projection(Some(23))),
            Some(retained_projection(Some(29))),
        );

        assert_eq!(first.semantic_view(), second.semantic_view());
    }

    #[test]
    fn proposal_semantic_view_ignores_projection_reuse_key_drift() {
        let first = InFlightProposal::noop(
            ProposalId::new(41),
            ScenePatch::derive(PatchBasis::new(
                Some(retained_projection(Some(11))),
                Some(retained_projection(Some(17))),
            )),
            RenderCleanupAction::Invalidate,
            RenderSideEffects {
                cursor_visibility: CursorVisibilityEffect::Hide,
                ..RenderSideEffects::default()
            },
            crate::core::state::AnimationSchedule::DefaultDelay,
        )
        .expect("noop proposal should preserve semantic patch shape");
        let second = InFlightProposal::noop(
            ProposalId::new(41),
            ScenePatch::derive(PatchBasis::new(
                Some(retained_projection(Some(23))),
                Some(retained_projection(Some(29))),
            )),
            RenderCleanupAction::Invalidate,
            RenderSideEffects {
                cursor_visibility: CursorVisibilityEffect::Hide,
                ..RenderSideEffects::default()
            },
            crate::core::state::AnimationSchedule::DefaultDelay,
        )
        .expect("noop proposal should preserve semantic patch shape");

        assert_eq!(first.semantic_view(), second.semantic_view());
    }

    #[test]
    fn realization_ledger_semantic_view_ignores_projection_reuse_key_drift() {
        let first = RealizationLedger::acknowledge(Some(retained_projection(Some(11))));
        let second = RealizationLedger::acknowledge(Some(retained_projection(Some(29))));

        assert_eq!(first.semantic_view(), second.semantic_view());
    }

    #[test]
    fn core_state_semantic_view_ignores_runtime_particle_cache_materialization() {
        let cold_runtime = particle_runtime();
        let mut warm_runtime = cold_runtime.clone();
        let _ = warm_runtime.shared_particle_screen_cells();

        let cold = CoreState::default().with_runtime(cold_runtime);
        let warm = CoreState::default().with_runtime(warm_runtime);

        assert_eq!(cold.semantic_view(), warm.semantic_view());
    }

    #[test]
    fn core_state_semantic_view_ignores_realization_projection_cache_drift() {
        let first = CoreState::default().with_realization(RealizationLedger::acknowledge(Some(
            retained_projection(Some(11)),
        )));
        let second = CoreState::default().with_realization(RealizationLedger::acknowledge(Some(
            retained_projection(Some(29)),
        )));

        assert_eq!(first.semantic_view(), second.semantic_view());
    }

    #[test]
    fn core_state_semantic_view_ignores_observing_prepared_plan_cache() {
        let request = observation_request(41);
        let observation = observation_snapshot(41);
        let without_cache = CoreState::default()
            .into_primed()
            .enter_observing_request(request)
            .expect("primed state should accept a collecting observation request")
            .with_active_observation(observation)
            .expect("collecting phase should accept the active observation");
        let mut with_cache = without_cache.clone();

        assert!(without_cache.prepared_observation_plan().is_none());
        assert!(with_cache.set_prepared_observation_plan(Some(prepared_observation_plan())));
        assert!(with_cache.prepared_observation_plan().is_some());
        assert_eq!(without_cache.semantic_view(), with_cache.semantic_view());
    }
}

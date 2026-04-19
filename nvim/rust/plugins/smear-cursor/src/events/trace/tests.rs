use super::core_state_summary;
use super::effect_summary;
use super::runtime_target_summary;
use crate::core::effect::ApplyProposalEffect;
use crate::core::effect::ApplyRenderCleanupEffect;
use crate::core::effect::Effect;
use crate::core::effect::RenderCleanupExecution;
use crate::core::effect::ScheduleTimerEffect;
use crate::core::realization::LogicalRaster;
use crate::core::runtime_reducer::CursorVisibilityEffect;
use crate::core::runtime_reducer::RenderCleanupAction;
use crate::core::runtime_reducer::RenderSideEffects;
use crate::core::runtime_reducer::TargetCellPresentation;
use crate::core::state::AnimationSchedule;
use crate::core::state::ApplyFailureKind;
use crate::core::state::BufferPerfClass;
use crate::core::state::CoreState;
use crate::core::state::DegradedApplyMetrics;
use crate::core::state::DemandQueue;
use crate::core::state::ExternalDemand;
use crate::core::state::ExternalDemandKind;
use crate::core::state::InFlightProposal;
use crate::core::state::ObservationBasis;
use crate::core::state::ObservationMotion;
use crate::core::state::ObservationSnapshot;
use crate::core::state::PatchBasis;
use crate::core::state::PendingObservation;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeRequestSet;
use crate::core::state::ProjectionHandle;
use crate::core::state::ProjectionReuseKey;
use crate::core::state::ProjectionWitness;
use crate::core::state::QueuedDemand;
use crate::core::state::RealizationDivergence;
use crate::core::state::RealizationFailure;
use crate::core::state::RealizationLedger;
use crate::core::state::RenderCleanupState;
use crate::core::state::RetainedProjection;
use crate::core::state::ScenePatch;
use crate::core::state::TimerState;
use crate::core::types::DelayBudgetMs;
use crate::core::types::IngressSeq;
use crate::core::types::Millis;
use crate::core::types::MotionRevision;
use crate::core::types::ObservationId;
use crate::core::types::ProjectionPolicyRevision;
use crate::core::types::ProjectorRevision;
use crate::core::types::ProposalId;
use crate::core::types::RenderRevision;
use crate::core::types::SemanticRevision;
use crate::core::types::TimerGeneration;
use crate::core::types::TimerId;
use crate::core::types::TimerToken;
use crate::draw::render_plan::CellOp;
use crate::draw::render_plan::Glyph;
use crate::draw::render_plan::HighlightLevel;
use crate::draw::render_plan::HighlightRef;
use crate::draw::render_plan::PlannerState;
use crate::position::BufferLine;
use crate::position::CursorObservation;
use crate::position::ObservedCell;
use crate::position::RenderPoint;
use crate::position::ScreenCell;
use crate::position::SurfaceId;
use crate::position::ViewportBounds;
use crate::position::WindowSurfaceSnapshot;
use crate::state::RuntimeState;
use crate::state::TrackedCursor;
use crate::types::CursorCellShape;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use std::sync::Arc;

fn cursor_position(row: u32, col: u32) -> ScreenCell {
    ScreenCell::new(i64::from(row), i64::from(col)).expect("positive cursor position")
}

fn viewport() -> ViewportBounds {
    ViewportBounds::new(40, 120).expect("positive viewport bounds")
}

fn external_demand(
    seq: u64,
    kind: ExternalDemandKind,
    observed_at: u64,
    buffer_perf_class: BufferPerfClass,
) -> ExternalDemand {
    ExternalDemand::new(
        IngressSeq::new(seq),
        kind,
        Millis::new(observed_at),
        buffer_perf_class,
    )
}

fn pending_observation(
    seq: u64,
    kind: ExternalDemandKind,
    observed_at: u64,
    requested_probes: ProbeRequestSet,
) -> PendingObservation {
    PendingObservation::new(
        external_demand(seq, kind, observed_at, BufferPerfClass::FastMotion),
        requested_probes,
    )
}

fn observation_snapshot(
    seq: u64,
    requested_probes: ProbeRequestSet,
    buffer_revision: Option<u64>,
) -> ObservationSnapshot {
    let basis = ObservationBasis::new(
        Millis::new(144),
        "n".to_string(),
        WindowSurfaceSnapshot::new(
            SurfaceId::new(17, 29).expect("positive handles"),
            BufferLine::new(4).expect("positive top buffer line"),
            2,
            1,
            ScreenCell::new(1, 3).expect("one-based window origin"),
            viewport(),
        ),
        CursorObservation::new(
            BufferLine::new(8).expect("positive buffer line"),
            ObservedCell::Exact(cursor_position(8, 16)),
        ),
        viewport(),
    )
    .with_buffer_revision(buffer_revision);
    ObservationSnapshot::new(
        pending_observation(seq, ExternalDemandKind::ModeChanged, 144, requested_probes),
        basis,
        ObservationMotion::new(None),
    )
}

fn retained_projection(
    ingress_seq: u64,
    render_revision: RenderRevision,
    cell_specs: &[(i64, i64, u32)],
) -> RetainedProjection {
    let cells = cell_specs
        .iter()
        .map(|&(row, col, zindex)| CellOp {
            row,
            col,
            zindex,
            glyph: Glyph::BLOCK,
            highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(2)),
        })
        .collect::<Vec<_>>();
    RetainedProjection::new(
        ProjectionWitness::new(
            render_revision,
            ObservationId::from_ingress_seq(IngressSeq::new(ingress_seq)),
            viewport(),
            ProjectorRevision::CURRENT,
        ),
        ProjectionReuseKey::new(
            Some(77),
            Some(88),
            None,
            TargetCellPresentation::None,
            ProjectionPolicyRevision::INITIAL,
        ),
        PlannerState::default(),
        LogicalRaster::new(None, Arc::from(cells)),
    )
}

fn demand_queue() -> DemandQueue {
    let (queue, _) = DemandQueue::default().enqueue(QueuedDemand::ready(external_demand(
        4,
        ExternalDemandKind::ExternalCursor,
        90,
        BufferPerfClass::Full,
    )));
    let (queue, _) = queue.enqueue(QueuedDemand::ready(external_demand(
        5,
        ExternalDemandKind::BoundaryRefresh,
        95,
        BufferPerfClass::Skip,
    )));
    queue
}

fn tracked_cursor() -> TrackedCursor {
    TrackedCursor::fixture(17, 29, 4, 8)
        .with_viewport_columns(2, 1)
        .with_window_origin(1, 3)
        .with_window_dimensions(120, 40)
}

fn runtime_state(max_kept_windows: usize) -> RuntimeState {
    let mut runtime = RuntimeState::default();
    runtime.config.max_kept_windows = max_kept_windows;
    runtime.initialize_cursor(
        RenderPoint::from(cursor_position(7, 15)),
        crate::state::CursorShape::block(),
        7,
        &tracked_cursor(),
    );
    runtime
}

fn runtime_state_with_minimal_tracked_cursor() -> RuntimeState {
    let mut runtime = RuntimeState::default();
    runtime.initialize_cursor(
        RenderPoint::from(cursor_position(7, 15)),
        crate::state::CursorShape::block(),
        7,
        &TrackedCursor::fixture(17, 29, 4, 8),
    );
    runtime
}

fn failure_metrics() -> DegradedApplyMetrics {
    DegradedApplyMetrics::new(9, 7, 2, 3, 4, 5, 6)
}

fn applying_proposal(acknowledged: ProjectionHandle, target: ProjectionHandle) -> InFlightProposal {
    InFlightProposal::failure(
        ProposalId::new(9),
        ScenePatch::derive(PatchBasis::new(Some(acknowledged), Some(target))),
        RealizationFailure::new(
            ApplyFailureKind::ViewportDrift,
            RealizationDivergence::ApplyMetrics(failure_metrics()),
        ),
        RenderCleanupAction::Invalidate,
        RenderSideEffects {
            redraw_after_draw_if_cmdline: true,
            redraw_after_clear_if_cmdline: false,
            target_cell_presentation: TargetCellPresentation::OverlayCursorCell(
                CursorCellShape::VerticalBar,
            ),
            cursor_visibility: CursorVisibilityEffect::Hide,
            allow_real_cursor_updates: false,
        },
        AnimationSchedule::Deadline(Millis::new(360)),
    )
}

#[test]
fn trace_summary_snapshot_renders_phase_owned_state_and_effects() {
    let retained = observation_snapshot(
        6,
        ProbeRequestSet::none().with_requested(ProbeKind::CursorColor),
        Some(41),
    );
    let (collecting_timers, _) = TimerState::default().arm(TimerId::Animation);
    let (collecting_timers, _) = collecting_timers.arm(TimerId::Cleanup);
    let collecting_state = CoreState::default()
        .with_runtime(runtime_state(5))
        .into_primed()
        .with_render_cleanup(RenderCleanupState::scheduled(Millis::new(120), 30, 90))
        .with_timers(collecting_timers)
        .with_demand_queue(demand_queue())
        .with_ready_observation(retained)
        .expect("primed state should accept a ready observation")
        .enter_observing_request(pending_observation(
            7,
            ExternalDemandKind::ModeChanged,
            120,
            ProbeRequestSet::none().with_requested(ProbeKind::CursorColor),
        ))
        .expect("ready state should stage a collecting observation request");
    let recovering_state = collecting_state.clone().enter_recovering();

    let acknowledged = retained_projection(
        8,
        RenderRevision::new(MotionRevision::INITIAL, SemanticRevision::INITIAL.next()),
        &[(6, 14, 40)],
    )
    .into_handle();
    let target = retained_projection(
        9,
        RenderRevision::new(
            MotionRevision::INITIAL.next(),
            SemanticRevision::INITIAL.next(),
        ),
        &[(6, 14, 40), (6, 15, 40)],
    )
    .into_handle();
    let proposal = applying_proposal(acknowledged.clone(), target);
    let (applying_timers, _) = TimerState::default().arm(TimerId::Ingress);
    let (applying_timers, _) = applying_timers.arm(TimerId::Recovery);
    let applying_state = CoreState::default()
        .with_runtime(runtime_state(5))
        .into_primed()
        .with_render_cleanup(
            RenderCleanupState::scheduled(Millis::new(150), 30, 90).enter_cooling(Millis::new(210)),
        )
        .with_timers(applying_timers)
        .with_demand_queue(demand_queue())
        .with_realization(RealizationLedger::diverged_from(
            Some(acknowledged),
            RealizationDivergence::ApplyMetrics(failure_metrics()),
        ))
        .with_ready_observation(observation_snapshot(
            9,
            ProbeRequestSet::none()
                .with_requested(ProbeKind::CursorColor)
                .with_requested(ProbeKind::Background),
            Some(44),
        ))
        .expect("primed state should accept a ready observation")
        .enter_planning(proposal.proposal_id())
        .expect("ready state should accept a planning proposal")
        .enter_applying(proposal.clone())
        .expect("planning state should accept the matching applying proposal");

    let apply_effect = Effect::ApplyProposal(Box::new(ApplyProposalEffect {
        proposal,
        buffer_handle: Some(29),
        requested_at: Millis::new(361),
    }));
    let schedule_timer_effect = Effect::ScheduleTimer(ScheduleTimerEffect {
        token: TimerToken::new(TimerId::Cleanup, TimerGeneration::new(5)),
        delay: DelayBudgetMs::try_new(24).expect("positive delay budget"),
        requested_at: Millis::new(362),
    });
    let cleanup_effect = Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
        execution: RenderCleanupExecution::CompactToBudget {
            target_budget: 2,
            max_prune_per_tick: 5,
        },
    });

    let snapshot = [
        format!("collecting={}", core_state_summary(&collecting_state)),
        format!("recovering={}", core_state_summary(&recovering_state)),
        format!("applying={}", core_state_summary(&applying_state)),
        format!("effect.apply={}", effect_summary(&apply_effect)),
        format!(
            "effect.schedule_timer={}",
            effect_summary(&schedule_timer_effect)
        ),
        format!("effect.cleanup={}", effect_summary(&cleanup_effect)),
    ]
    .join("\n");

    assert_snapshot!(snapshot);
}

#[test]
fn runtime_target_summary_renders_minimal_canonical_tracked_surface() {
    assert_eq!(
        runtime_target_summary(&runtime_state_with_minimal_tracked_cursor()),
        "cell=7:15 epoch=1 tracked=(surface=(id=(win=17 buf=29) top=4 left=0 textoff=0 origin=1:1 size=1x1) buffer_line=8)"
    );
}

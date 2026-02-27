use super::reduce;
use crate::core::effect::{
    ApplyRenderCleanupEffect, CursorPositionReadPolicy, Effect, EventLoopMetricEffect,
    IngressCursorPresentationEffect, IngressCursorPresentationRequest, ObservationRuntimeContext,
    RenderCleanupExecution, RequestObservationBaseEffect, RequestProbeEffect, ScheduleTimerEffect,
    TimerKind,
};
use crate::core::event::{
    ApplyReport, EffectFailedEvent, Event, ExternalDemandQueuedEvent, InitializeEvent,
    KeyFallbackQueuedEvent, ObservationBaseCollectedEvent, ProbeReportedEvent,
    RenderCleanupAppliedAction, RenderCleanupAppliedEvent, RenderPlanComputedEvent,
    TimerFiredWithTokenEvent, TimerLostWithTokenEvent,
};
use crate::core::runtime_reducer::{
    RenderCleanupAction, RenderSideEffects, render_cleanup_delay_ms, render_hard_cleanup_delay_ms,
};
use crate::core::state::{
    BackgroundProbeBatch, BackgroundProbeChunk, CoreState, CursorColorSample, DegradedApplyMetrics,
    DemandQueue, ExternalDemand, ExternalDemandKind, InFlightProposal, IngressPolicyState,
    ObservationBasis, ObservationMotion, ObservationRequest, ObservationSnapshot, PatchBasis,
    ProbeFailure, ProbeKind, ProbeRequestSet, ProbeReuse, ProbeSet, ProbeState, RealizationClear,
    RealizationDivergence, RealizationLedger, RealizationPlan, RecoveryPolicyState,
    RenderCleanupState, ScenePatch, SemanticEntityId,
};
use crate::core::types::{
    CursorCol, CursorPosition, CursorRow, DelayBudgetMs, IngressSeq, Lifecycle, Millis, ProposalId,
    TimerGeneration, TimerId, TimerToken, ViewportSnapshot,
};
use crate::state::{CursorLocation, CursorShape};
use crate::types::{Point, ScreenCell};

fn cursor(row: u32, col: u32) -> CursorPosition {
    CursorPosition {
        row: CursorRow(row),
        col: CursorCol(col),
    }
}

fn noop_realization_plan() -> RealizationPlan {
    RealizationPlan::Noop
}

fn with_cleanup_invalidation(effects: Vec<Effect>) -> Vec<Effect> {
    effects
}

fn ready_state() -> CoreState {
    CoreState::default().initialize()
}

fn observation_request(seq: u64, kind: ExternalDemandKind, observed_at: u64) -> ObservationRequest {
    ObservationRequest::new(
        ExternalDemand::new(IngressSeq::new(seq), kind, Millis::new(observed_at), None),
        ProbeRequestSet::default(),
    )
}

fn observation_basis(
    request: &ObservationRequest,
    position: Option<CursorPosition>,
    observed_at: u64,
) -> ObservationBasis {
    ObservationBasis::new(
        request.observation_id(),
        Millis::new(observed_at),
        "n".to_string(),
        position,
        CursorLocation::new(11, 22, 3, 4),
        ViewportSnapshot::new(CursorRow(40), CursorCol(120)),
    )
}

fn observation_motion() -> ObservationMotion {
    ObservationMotion::default()
}

fn observation_snapshot(position: CursorPosition) -> ObservationSnapshot {
    let request = observation_request(9, ExternalDemandKind::ExternalCursor, 90);
    let basis = observation_basis(&request, Some(position), 91);
    ObservationSnapshot::new(request, basis, ProbeSet::default(), observation_motion())
}

fn cursor_position_policy(state: &CoreState) -> CursorPositionReadPolicy {
    CursorPositionReadPolicy::new(state.runtime().config.smear_to_cmd)
}

fn observation_runtime_context(state: &CoreState) -> ObservationRuntimeContext {
    ObservationRuntimeContext::new(
        cursor_position_policy(state),
        state.runtime().config.scroll_buffer_space,
        state.runtime().tracked_location(),
        state.runtime().current_corners(),
    )
}

fn cursor_color_probe_report(
    request: &ObservationRequest,
    reuse: ProbeReuse,
    color: Option<&str>,
) -> Event {
    Event::ProbeReported(ProbeReportedEvent::CursorColorReady {
        observation_id: request.observation_id(),
        probe_request_id: ProbeKind::CursorColor.request_id(request.observation_id()),
        reuse,
        sample: color.map(|value| CursorColorSample::new(value.to_string())),
    })
}

fn cursor_color_probe_failed(request: &ObservationRequest) -> Event {
    Event::ProbeReported(ProbeReportedEvent::CursorColorFailed {
        observation_id: request.observation_id(),
        probe_request_id: ProbeKind::CursorColor.request_id(request.observation_id()),
        failure: ProbeFailure::ShellReadFailed,
    })
}

fn background_probe_batch(
    viewport: ViewportSnapshot,
    allowed_cells: &[(u32, u32)],
) -> BackgroundProbeBatch {
    let width = usize::try_from(viewport.max_col.value()).expect("viewport width");
    let height = usize::try_from(viewport.max_row.value()).expect("viewport height");
    let mut allowed_mask = vec![false; width * height];
    for &(row, col) in allowed_cells {
        let row_index = usize::try_from(row.saturating_sub(1)).expect("row index");
        let col_index = usize::try_from(col.saturating_sub(1)).expect("col index");
        let index = row_index * width + col_index;
        allowed_mask[index] = true;
    }

    BackgroundProbeBatch::from_allowed_mask(viewport, allowed_mask)
}

fn background_probe_report(
    request: &ObservationRequest,
    viewport: ViewportSnapshot,
    allowed_cells: &[(u32, u32)],
    reuse: ProbeReuse,
) -> Event {
    Event::ProbeReported(ProbeReportedEvent::BackgroundReady {
        observation_id: request.observation_id(),
        probe_request_id: ProbeKind::Background.request_id(request.observation_id()),
        reuse,
        batch: background_probe_batch(viewport, allowed_cells),
    })
}

fn background_chunk_probe_report(
    request: &ObservationRequest,
    chunk: BackgroundProbeChunk,
    viewport: ViewportSnapshot,
    allowed_cells: &[(u32, u32)],
) -> Event {
    let width = usize::try_from(viewport.max_col.value()).expect("viewport width");
    let row_count = usize::try_from(chunk.row_count()).expect("chunk row count");
    let mut allowed_mask = vec![false; width * row_count];
    let start_row = chunk.start_row().value();
    let end_row = start_row.saturating_add(chunk.row_count().saturating_sub(1));

    for &(row, col) in allowed_cells {
        if row < start_row || row > end_row {
            continue;
        }
        let row_index = usize::try_from(row.saturating_sub(start_row)).expect("row index");
        let col_index = usize::try_from(col.saturating_sub(1)).expect("col index");
        let index = row_index * width + col_index;
        allowed_mask[index] = true;
    }

    Event::ProbeReported(ProbeReportedEvent::BackgroundChunkReady {
        observation_id: request.observation_id(),
        probe_request_id: ProbeKind::Background.request_id(request.observation_id()),
        chunk,
        allowed_mask,
    })
}

fn ready_state_with_observation(position: CursorPosition) -> CoreState {
    ready_state()
        .with_last_cursor(Some(position))
        .into_ready_with_observation(observation_snapshot(position))
}

fn recovering_state_with_observation(position: CursorPosition) -> CoreState {
    ready_state_with_observation(position).into_recovering()
}

fn timer_armed_state(state: CoreState) -> (CoreState, TimerToken) {
    let (timers, token) = state.timers().arm(TimerId::Animation);
    (state.with_timers(timers), token)
}

fn animation_tick_event(token: TimerToken, observed_at: u64) -> Event {
    Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
        kind: TimerKind::Animation,
        token,
        observed_at: Millis::new(observed_at),
    })
}

fn ingress_tick_event(token: TimerToken, observed_at: u64) -> Event {
    Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
        kind: TimerKind::Ingress,
        token,
        observed_at: Millis::new(observed_at),
    })
}

fn cleanup_tick_event(token: TimerToken, observed_at: u64) -> Event {
    Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
        kind: TimerKind::Cleanup,
        token,
        observed_at: Millis::new(observed_at),
    })
}

fn planned_state_after_animation_tick(
    state: CoreState,
    observed_at: u64,
) -> (CoreState, ProposalId) {
    let (armed_state, token) = timer_armed_state(state);
    let transition = reduce(&armed_state, animation_tick_event(token, observed_at));
    let Effect::RequestRenderPlan(payload) = transition
        .effects
        .into_iter()
        .next()
        .expect("render plan request after animation tick")
    else {
        panic!("expected render plan request after animation tick");
    };
    let proposal_id = payload.proposal_id;
    let computed = reduce(
        &transition.next,
        Event::RenderPlanComputed(RenderPlanComputedEvent {
            proposal_id,
            planned_render: crate::core::reducer::build_planned_render(
                &payload.planning_state,
                payload.proposal_id,
                &payload.render_decision,
                payload.should_schedule_next_animation,
                payload.next_animation_at_ms,
            ),
            observed_at: payload.requested_at,
        }),
    );
    (computed.next, proposal_id)
}

fn applying_state_with_realization_plan(
    state: CoreState,
    realization: RealizationPlan,
    should_schedule_next_animation: bool,
    next_animation_at_ms: Option<Millis>,
) -> (CoreState, ProposalId) {
    let basis = PatchBasis::new(
        state
            .realization()
            .trusted_acknowledged_for_patch()
            .cloned(),
        None,
    );
    let patch = ScenePatch::derive(basis.clone());
    let (state, proposal_id) = state.allocate_proposal_id();
    let proposal = InFlightProposal::new(
        proposal_id,
        basis,
        patch,
        realization,
        RenderCleanupAction::NoAction,
        RenderSideEffects::default(),
        should_schedule_next_animation,
        next_animation_at_ms,
    );
    (
        state
            .into_applying(proposal)
            .expect("test staging requires a retained observation"),
        proposal_id,
    )
}

#[test]
fn initialize_from_idle_enters_primed_protocol_without_follow_up_reads() {
    let state = CoreState::default();

    let transition = reduce(
        &state,
        Event::Initialize(InitializeEvent {
            observed_at: Millis::new(11),
        }),
    );

    assert_eq!(transition.next.lifecycle(), Lifecycle::Primed);
    assert!(transition.effects.is_empty());
}

#[test]
fn lifecycle_constructors_preserve_protocol_owned_shared_state() {
    let recovery_policy = RecoveryPolicyState::default().with_retry_attempt(3);
    let ingress_policy = IngressPolicyState::default().note_cursor_autocmd(Millis::new(55));
    let (timers, armed_token) = CoreState::default().timers().arm(TimerId::Animation);
    let primed = CoreState::default()
        .with_timers(timers)
        .with_recovery_policy(recovery_policy)
        .with_ingress_policy(ingress_policy)
        .initialize();

    assert_eq!(primed.timers(), timers);
    assert_eq!(
        primed.timers().active_token(TimerId::Animation),
        Some(armed_token)
    );
    assert_eq!(primed.recovery_policy(), recovery_policy);
    assert_eq!(primed.ingress_policy(), ingress_policy);

    let request = observation_request(11, ExternalDemandKind::ExternalCursor, 77);
    let observing = primed.into_observing(request, DemandQueue::default());
    assert_eq!(observing.timers(), timers);
    assert_eq!(observing.recovery_policy(), recovery_policy);
    assert_eq!(observing.ingress_policy(), ingress_policy);

    let observation = observation_snapshot(cursor(4, 9));
    let ready = observing
        .with_active_observation(Some(observation.clone()))
        .expect("observation staging should succeed")
        .into_ready_with_observation(observation);
    assert_eq!(ready.timers(), timers);
    assert_eq!(ready.recovery_policy(), recovery_policy);
    assert_eq!(ready.ingress_policy(), ingress_policy);

    let recovering = ready.clone().into_recovering();
    assert_eq!(recovering.timers(), timers);
    assert_eq!(recovering.recovery_policy(), recovery_policy);
    assert_eq!(recovering.ingress_policy(), ingress_policy);

    let (applying, proposal_id) =
        applying_state_with_realization_plan(ready, noop_realization_plan(), false, None);
    assert_eq!(applying.timers(), timers);
    assert_eq!(applying.recovery_policy(), recovery_policy);
    assert_eq!(applying.ingress_policy(), ingress_policy);

    let (cleared, _) = applying
        .clear_pending_for(proposal_id)
        .expect("proposal should clear back to ready");
    assert_eq!(cleared.timers(), timers);
    assert_eq!(cleared.recovery_policy(), recovery_policy);
    assert_eq!(cleared.ingress_policy(), ingress_policy);
}

#[test]
fn cursor_demand_is_queue_owned_and_coalesced_while_observing() {
    let ready = ready_state();
    let first = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(20),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    );

    assert_eq!(first.next.lifecycle(), Lifecycle::Observing);
    assert_eq!(
        first.effects,
        with_cleanup_invalidation(vec![Effect::RequestObservationBase(
            RequestObservationBaseEffect {
                request: observation_request(1, ExternalDemandKind::ExternalCursor, 20),
                context: observation_runtime_context(&ready),
            }
        )])
    );

    let second = reduce(
        &first.next,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(21),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    );

    assert_eq!(second.next.lifecycle(), Lifecycle::Observing);
    assert_eq!(
        second.effects,
        vec![Effect::RecordEventLoopMetric(
            EventLoopMetricEffect::IngressCoalesced,
        )]
    );

    let third = reduce(
        &second.next,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(22),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    );

    let queued_cursor = third
        .next
        .demand_queue()
        .latest_cursor()
        .expect("queued cursor demand");
    assert_eq!(
        queued_cursor,
        &crate::core::state::QueuedDemand::Ready(ExternalDemand::new(
            IngressSeq::new(3),
            ExternalDemandKind::ExternalCursor,
            Millis::new(22),
            None,
        ))
    );
}

#[test]
fn observation_request_uses_explicit_cursor_color_probe_policy() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.cursor_color = Some("none".to_string());
    let ready = ready_state().with_runtime(runtime);

    let transition = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(24),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    );

    assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    assert_eq!(
        transition.effects,
        with_cleanup_invalidation(vec![Effect::RequestObservationBase(
            RequestObservationBaseEffect {
                request: ObservationRequest::new(
                    ExternalDemand::new(
                        IngressSeq::new(1),
                        ExternalDemandKind::ExternalCursor,
                        Millis::new(24),
                        None,
                    ),
                    ProbeRequestSet::new(true, false),
                ),
                context: observation_runtime_context(&ready),
            }
        )])
    );
}

#[test]
fn observation_request_uses_explicit_background_probe_policy() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.particles_enabled = true;
    runtime.config.particles_over_text = false;
    let ready = ready_state().with_runtime(runtime);

    let transition = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(24),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    );

    assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    assert_eq!(
        transition.effects,
        with_cleanup_invalidation(vec![Effect::RequestObservationBase(
            RequestObservationBaseEffect {
                request: ObservationRequest::new(
                    ExternalDemand::new(
                        IngressSeq::new(1),
                        ExternalDemandKind::ExternalCursor,
                        Millis::new(24),
                        None,
                    ),
                    ProbeRequestSet::new(false, true),
                ),
                context: observation_runtime_context(&ready),
            }
        )])
    );
}

#[test]
fn observation_request_captures_runtime_tracking_context() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.scroll_buffer_space = false;
    runtime.config.smear_to_cmd = false;
    runtime.initialize_cursor(
        Point {
            row: 12.0,
            col: 18.0,
        },
        CursorShape::new(false, false),
        7,
        CursorLocation::new(44, 55, 9, 10),
    );
    let ready = ready_state().with_runtime(runtime);

    let transition = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(24),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    );

    assert_eq!(
        transition.effects,
        with_cleanup_invalidation(vec![Effect::RequestObservationBase(
            RequestObservationBaseEffect {
                request: observation_request(1, ExternalDemandKind::ExternalCursor, 24),
                context: observation_runtime_context(&ready),
            }
        )])
    );
}

#[test]
fn observation_base_collection_emits_cursor_color_probe_request() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.cursor_color = Some("none".to_string());
    let ready = ready_state().with_runtime(runtime);
    let observing = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(25),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    )
    .next;
    let request = observing
        .active_observation_request()
        .cloned()
        .expect("active observation");

    let based = reduce(
        &observing,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis: observation_basis(&request, Some(cursor(7, 8)), 26),
            motion: observation_motion(),
        }),
    );

    assert_eq!(based.next.lifecycle(), Lifecycle::Observing);
    assert_eq!(
        based.effects,
        vec![Effect::RequestProbe(RequestProbeEffect {
            observation_basis: observation_basis(&request, Some(cursor(7, 8)), 26),
            probe_request_id: ProbeKind::CursorColor.request_id(request.observation_id()),
            kind: ProbeKind::CursorColor,
            cursor_position_policy: cursor_position_policy(&observing),
            background_chunk: None,
        })]
    );
    match based
        .next
        .observation()
        .expect("active observation snapshot")
        .probes()
        .cursor_color()
    {
        ProbeState::Pending { .. } => {}
        other => panic!("expected pending cursor color probe, got {other:?}"),
    }
}

#[test]
fn observation_base_collection_stages_only_first_pending_probe_when_multiple_probes_are_requested()
{
    let mut runtime = ready_state().runtime().clone();
    runtime.config.cursor_color = Some("none".to_string());
    runtime.config.particles_enabled = true;
    runtime.config.particles_over_text = false;
    let ready = ready_state().with_runtime(runtime);
    let observing = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(25),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    )
    .next;
    let request = observing
        .active_observation_request()
        .cloned()
        .expect("active observation");
    let basis = observation_basis(&request, Some(cursor(7, 8)), 26);

    let based = reduce(
        &observing,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis: basis.clone(),
            motion: observation_motion(),
        }),
    );

    assert_eq!(based.next.lifecycle(), Lifecycle::Observing);
    assert_eq!(
        based.effects,
        vec![Effect::RequestProbe(RequestProbeEffect {
            observation_basis: basis,
            probe_request_id: ProbeKind::CursorColor.request_id(request.observation_id()),
            kind: ProbeKind::CursorColor,
            cursor_position_policy: cursor_position_policy(&observing),
            background_chunk: None,
        })]
    );
    let observation = based
        .next
        .observation()
        .expect("active observation snapshot");
    assert!(matches!(
        observation.probes().cursor_color(),
        ProbeState::Pending { .. }
    ));
    assert!(matches!(
        observation.probes().background(),
        ProbeState::Pending { .. }
    ));
}

#[test]
fn compatible_probe_report_stores_cursor_color_probe_in_snapshot() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.cursor_color = Some("none".to_string());
    let ready = ready_state().with_runtime(runtime);
    let observing = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(25),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    )
    .next;
    let request = observing
        .active_observation_request()
        .cloned()
        .expect("active observation");
    let based = reduce(
        &observing,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis: observation_basis(&request, Some(cursor(7, 8)), 26),
            motion: observation_motion(),
        }),
    );

    let completed = reduce(
        &based.next,
        cursor_color_probe_report(&request, ProbeReuse::Compatible, Some("#abcdef")),
    );

    let observation = completed
        .next
        .observation()
        .expect("stored observation snapshot");
    assert_eq!(observation.cursor_color(), Some("#abcdef"));
    match observation.probes().cursor_color() {
        ProbeState::Ready { reuse, .. } => assert_eq!(*reuse, ProbeReuse::Compatible),
        other => panic!("expected ready cursor color probe, got {other:?}"),
    }
}

#[test]
fn cursor_color_probe_completion_stages_background_probe_before_apply_when_both_probes_are_requested()
 {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.cursor_color = Some("none".to_string());
    runtime.config.particles_enabled = true;
    runtime.config.particles_over_text = false;
    let ready = ready_state().with_runtime(runtime);
    let observing = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(25),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    )
    .next;
    let request = observing
        .active_observation_request()
        .cloned()
        .expect("active observation");
    let basis = observation_basis(&request, Some(cursor(7, 8)), 26);
    let based = reduce(
        &observing,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis: basis.clone(),
            motion: observation_motion(),
        }),
    );

    let after_cursor = reduce(
        &based.next,
        cursor_color_probe_report(&request, ProbeReuse::Compatible, Some("#abcdef")),
    );
    let first_background_chunk = after_cursor
        .next
        .observation()
        .and_then(|observation| observation.background_progress())
        .and_then(crate::core::state::BackgroundProbeProgress::next_chunk)
        .expect("first background probe chunk");

    assert_eq!(after_cursor.next.lifecycle(), Lifecycle::Observing);
    assert!(after_cursor.next.pending_proposal().is_none());
    assert_eq!(
        after_cursor.effects,
        vec![Effect::RequestProbe(RequestProbeEffect {
            observation_basis: basis.clone(),
            probe_request_id: ProbeKind::Background.request_id(request.observation_id()),
            kind: ProbeKind::Background,
            cursor_position_policy: cursor_position_policy(&based.next),
            background_chunk: Some(first_background_chunk),
        })]
    );
    let observation = after_cursor
        .next
        .observation()
        .expect("observation should stay active while background probe is pending");
    assert_eq!(observation.cursor_color(), Some("#abcdef"));
    assert!(matches!(
        observation.probes().background(),
        ProbeState::Pending { .. }
    ));

    let after_background = reduce(
        &after_cursor.next,
        background_probe_report(&request, basis.viewport(), &[(7, 8)], ProbeReuse::Exact),
    );

    assert_eq!(after_background.next.lifecycle(), Lifecycle::Planning);
    assert!(after_background.next.pending_proposal().is_none());
    assert!(after_background.next.pending_plan_proposal_id().is_some());
    assert!(matches!(
        after_background.effects.as_slice(),
        [Effect::RequestRenderPlan(_)]
    ));
}

#[test]
fn observation_result_stores_background_probe_in_snapshot() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.particles_enabled = true;
    runtime.config.particles_over_text = false;
    let ready = ready_state().with_runtime(runtime);
    let observing = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(25),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    )
    .next;
    let request = observing
        .active_observation_request()
        .cloned()
        .expect("active observation");
    let basis = observation_basis(&request, Some(cursor(7, 8)), 26);
    let completed = reduce(
        &observing,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis: basis.clone(),
            motion: observation_motion(),
        }),
    );

    assert_eq!(completed.next.lifecycle(), Lifecycle::Observing);
    assert_eq!(
        completed.effects,
        vec![Effect::RequestProbe(RequestProbeEffect {
            observation_basis: basis.clone(),
            probe_request_id: ProbeKind::Background.request_id(request.observation_id()),
            kind: ProbeKind::Background,
            cursor_position_policy: cursor_position_policy(&observing),
            background_chunk: completed
                .next
                .observation()
                .and_then(|observation| observation.background_progress())
                .and_then(crate::core::state::BackgroundProbeProgress::next_chunk),
        })]
    );

    let resolved = reduce(
        &completed.next,
        background_probe_report(&request, basis.viewport(), &[(7, 8)], ProbeReuse::Exact),
    );

    let observation = resolved
        .next
        .observation()
        .expect("stored observation snapshot");
    let background = observation
        .background_probe()
        .expect("background probe batch");
    assert!(background.allows_particle(crate::types::ScreenCell::new(7, 8).expect("cell")));
    assert!(!background.allows_particle(crate::types::ScreenCell::new(7, 9).expect("cell")));
    match observation.probes().background() {
        ProbeState::Ready { reuse, .. } => assert_eq!(*reuse, ProbeReuse::Exact),
        other => panic!("expected ready background probe, got {other:?}"),
    }
}

#[test]
fn background_chunk_probe_progress_stages_the_next_chunk_before_apply() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.particles_enabled = true;
    runtime.config.particles_over_text = false;
    let ready = ready_state().with_runtime(runtime);
    let observing = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(25),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    )
    .next;
    let request = observing
        .active_observation_request()
        .cloned()
        .expect("active observation");
    let basis = observation_basis(&request, Some(cursor(7, 8)), 26);
    let completed = reduce(
        &observing,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis: basis.clone(),
            motion: observation_motion(),
        }),
    );
    let first_chunk = completed
        .next
        .observation()
        .and_then(|observation| observation.background_progress())
        .and_then(crate::core::state::BackgroundProbeProgress::next_chunk)
        .expect("first chunk");

    let after_first_chunk = reduce(
        &completed.next,
        background_chunk_probe_report(&request, first_chunk, basis.viewport(), &[(7, 8)]),
    );

    let progressed_observation = after_first_chunk
        .next
        .observation()
        .expect("observation should remain active");
    let progressed = progressed_observation
        .background_progress()
        .expect("background progress after first chunk");
    let next_chunk = progressed.next_chunk().expect("second chunk");
    assert_eq!(
        progressed.next_row(),
        CursorRow(
            first_chunk
                .start_row()
                .value()
                .saturating_add(first_chunk.row_count())
        )
    );
    assert!(progressed_observation.background_probe().is_none());
    assert_eq!(after_first_chunk.next.lifecycle(), Lifecycle::Observing);
    assert!(after_first_chunk.next.pending_proposal().is_none());
    assert_eq!(
        after_first_chunk.effects,
        vec![Effect::RequestProbe(RequestProbeEffect {
            observation_basis: basis,
            probe_request_id: ProbeKind::Background.request_id(request.observation_id()),
            kind: ProbeKind::Background,
            cursor_position_policy: cursor_position_policy(&completed.next),
            background_chunk: Some(next_chunk),
        })]
    );
}

#[test]
fn refresh_required_probe_report_retries_base_observation() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.cursor_color = Some("none".to_string());
    let ready = ready_state().with_runtime(runtime);
    let observing = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(25),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    )
    .next;
    let request = observing
        .active_observation_request()
        .cloned()
        .expect("active observation");
    let based = reduce(
        &observing,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis: observation_basis(&request, Some(cursor(7, 8)), 26),
            motion: observation_motion(),
        }),
    );

    let retried = reduce(
        &based.next,
        cursor_color_probe_report(&request, ProbeReuse::RefreshRequired, None),
    );

    assert_eq!(retried.next.lifecycle(), Lifecycle::Observing);
    assert_eq!(
        retried.effects,
        vec![
            Effect::RecordEventLoopMetric(EventLoopMetricEffect::ProbeRefreshRetried(
                ProbeKind::CursorColor,
            )),
            Effect::RequestObservationBase(RequestObservationBaseEffect {
                request,
                context: observation_runtime_context(&based.next),
            })
        ]
    );
    assert!(retried.next.observation().is_none());
    assert_eq!(
        retried
            .next
            .probe_refresh_state()
            .expect("probe refresh state while observing")
            .retry_count(ProbeKind::CursorColor),
        1
    );
}

#[test]
fn refresh_required_probe_report_yields_to_newer_ingress_after_retry_budget_is_exhausted() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.cursor_color = Some("none".to_string());
    let ready = ready_state().with_runtime(runtime);
    let observing = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(25),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    )
    .next;
    let request = observing
        .active_observation_request()
        .cloned()
        .expect("active observation");
    let based = reduce(
        &observing,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis: observation_basis(&request, Some(cursor(7, 8)), 26),
            motion: observation_motion(),
        }),
    );

    let queued_newer = reduce(
        &based.next,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(27),
            requested_target: Some(cursor(9, 10)),
            ingress_cursor_presentation: None,
        }),
    )
    .next;

    let retry_once = reduce(
        &queued_newer,
        cursor_color_probe_report(&request, ProbeReuse::RefreshRequired, None),
    );
    assert!(matches!(
        retry_once.effects.as_slice(),
        [
            Effect::RecordEventLoopMetric(EventLoopMetricEffect::ProbeRefreshRetried(
                ProbeKind::CursorColor
            )),
            Effect::RequestObservationBase(_)
        ]
    ));

    let retry_once_based = reduce(
        &retry_once.next,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis: observation_basis(&request, Some(cursor(7, 9)), 28),
            motion: observation_motion(),
        }),
    );
    let retry_twice = reduce(
        &retry_once_based.next,
        cursor_color_probe_report(&request, ProbeReuse::RefreshRequired, None),
    );
    assert!(matches!(
        retry_twice.effects.as_slice(),
        [
            Effect::RecordEventLoopMetric(EventLoopMetricEffect::ProbeRefreshRetried(
                ProbeKind::CursorColor
            )),
            Effect::RequestObservationBase(_)
        ]
    ));

    let retry_twice_based = reduce(
        &retry_twice.next,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis: observation_basis(&request, Some(cursor(7, 10)), 29),
            motion: observation_motion(),
        }),
    );
    let exhausted = reduce(
        &retry_twice_based.next,
        cursor_color_probe_report(&request, ProbeReuse::RefreshRequired, None),
    );

    let replacement_request = exhausted
        .next
        .active_observation_request()
        .cloned()
        .expect("newer ingress should take over after retry budget exhaustion");
    assert_ne!(replacement_request, request);
    assert_eq!(
        replacement_request.demand().requested_target(),
        Some(cursor(9, 10))
    );
    assert_eq!(exhausted.next.lifecycle(), Lifecycle::Observing);
    assert_eq!(
        exhausted.effects,
        vec![
            Effect::RecordEventLoopMetric(EventLoopMetricEffect::ProbeRefreshBudgetExhausted(
                ProbeKind::CursorColor
            ),),
            Effect::RequestObservationBase(RequestObservationBaseEffect {
                request: replacement_request,
                context: observation_runtime_context(&exhausted.next),
            }),
        ]
    );
    assert!(exhausted.next.observation().is_none());
}

#[test]
fn failed_probe_report_is_retained_without_collapsing_to_missing() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.cursor_color = Some("none".to_string());
    let ready = ready_state().with_runtime(runtime);
    let observing = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(25),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    )
    .next;
    let request = observing
        .active_observation_request()
        .cloned()
        .expect("active observation");
    let based = reduce(
        &observing,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis: observation_basis(&request, Some(cursor(7, 8)), 26),
            motion: observation_motion(),
        }),
    );

    let completed = reduce(&based.next, cursor_color_probe_failed(&request));

    let observation = completed
        .next
        .observation()
        .expect("stored observation snapshot");
    assert_eq!(observation.cursor_color(), None);
    match observation.probes().cursor_color() {
        ProbeState::Failed { failure, .. } => assert_eq!(*failure, ProbeFailure::ShellReadFailed),
        other => panic!("expected failed cursor color probe, got {other:?}"),
    }
}

#[test]
fn observation_result_stages_render_plan_before_dequeuing_next_pending_demand() {
    let ready = ready_state();
    let observing = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ModeChanged,
            observed_at: Millis::new(30),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    )
    .next;
    let observing_with_cursor_queued = reduce(
        &observing,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(31),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    )
    .next;
    let request = observing_with_cursor_queued
        .active_observation_request()
        .cloned()
        .expect("active observation");

    let completed = reduce(
        &observing_with_cursor_queued,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis: observation_basis(&request, Some(cursor(7, 8)), 32),
            motion: observation_motion(),
        }),
    );

    assert_eq!(completed.next.lifecycle(), Lifecycle::Planning);
    assert_eq!(completed.effects.len(), 1);
    match &completed.effects[0] {
        Effect::RequestRenderPlan(payload) => {
            assert_eq!(payload.requested_at, Millis::new(32));
            assert_eq!(
                Some(payload.proposal_id),
                completed.next.pending_plan_proposal_id()
            );
        }
        other => panic!("expected first render plan request, got {other:?}"),
    }
    assert!(completed.next.demand_queue().latest_cursor().is_some());
}

#[test]
fn recent_autocmd_ingress_suppresses_key_fallback_in_reducer() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.delay_after_key = 25.0;
    let state = ready_state()
        .with_runtime(runtime)
        .with_ingress_policy(IngressPolicyState::default().note_cursor_autocmd(Millis::new(100)));

    let suppressed = reduce(
        &state,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::KeyFallback,
            observed_at: Millis::new(112),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    );

    assert_eq!(suppressed.next.lifecycle(), state.lifecycle());
    assert!(suppressed.next.demand_queue().latest_cursor().is_none());
    assert!(suppressed.next.demand_queue().ordered().is_empty());
    assert_eq!(
        suppressed.next.entropy().next_ingress_seq(),
        IngressSeq::new(1)
    );
    assert!(suppressed.effects.is_empty());
}

#[test]
fn stale_autocmd_ingress_allows_key_fallback_in_reducer() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.delay_after_key = 25.0;
    let state = ready_state()
        .with_runtime(runtime)
        .with_ingress_policy(IngressPolicyState::default().note_cursor_autocmd(Millis::new(100)));

    let admitted = reduce(
        &state,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::KeyFallback,
            observed_at: Millis::new(160),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    );

    assert_eq!(admitted.next.lifecycle(), Lifecycle::Observing);
    assert_eq!(
        admitted.effects,
        with_cleanup_invalidation(vec![Effect::RequestObservationBase(
            RequestObservationBaseEffect {
                request: observation_request(1, ExternalDemandKind::KeyFallback, 160),
                context: observation_runtime_context(&state),
            }
        )])
    );
}

#[test]
fn deferred_key_fallback_is_queued_and_arms_ingress_timer() {
    let transition = reduce(
        &ready_state(),
        Event::KeyFallbackQueued(KeyFallbackQueuedEvent {
            observed_at: Millis::new(120),
            due_at: Millis::new(145),
        }),
    );

    let expected_token = TimerToken::new(TimerId::Ingress, TimerGeneration::new(1));
    assert_eq!(transition.next.lifecycle(), Lifecycle::Primed);
    assert_eq!(
        transition.next.demand_queue().latest_cursor(),
        Some(&crate::core::state::QueuedDemand::PendingKeyFallback {
            seq: IngressSeq::new(1),
            due_at: Millis::new(145),
            requested_target: None,
        })
    );
    assert_eq!(
        transition.effects,
        with_cleanup_invalidation(vec![Effect::ScheduleTimer(ScheduleTimerEffect {
            kind: TimerKind::Ingress,
            token: expected_token,
            delay: DelayBudgetMs::try_new(25).expect("positive delay"),
            requested_at: Millis::new(120),
        })])
    );
}

#[test]
fn pending_key_fallback_keeps_armed_cleanup_live_through_suppression() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.delay_after_key = 25.0;
    let cleanup = RenderCleanupState::scheduled(Millis::new(90), 200, 3_000, 21);
    let (timers, cleanup_token) = ready_state().timers().arm(TimerId::Cleanup);
    let state = ready_state()
        .with_runtime(runtime)
        .with_timers(timers)
        .with_render_cleanup(cleanup)
        .with_ingress_policy(IngressPolicyState::default().note_cursor_autocmd(Millis::new(100)));

    let queued = reduce(
        &state,
        Event::KeyFallbackQueued(KeyFallbackQueuedEvent {
            observed_at: Millis::new(112),
            due_at: Millis::new(120),
        }),
    );
    let ingress_token = queued
        .next
        .timers()
        .active_token(TimerId::Ingress)
        .expect("pending key fallback should arm ingress timer");

    assert_eq!(queued.next.render_cleanup(), cleanup);
    assert_eq!(
        queued.next.timers().active_token(TimerId::Cleanup),
        Some(cleanup_token)
    );

    let suppressed = reduce(&queued.next, ingress_tick_event(ingress_token, 120));

    assert_eq!(suppressed.next.render_cleanup(), cleanup);
    assert_eq!(
        suppressed.next.timers().active_token(TimerId::Cleanup),
        Some(cleanup_token)
    );
    assert_eq!(
        suppressed.next.timers().active_token(TimerId::Ingress),
        None
    );
    assert!(suppressed.effects.is_empty());
}

#[test]
fn ingress_timer_fires_and_requests_deferred_key_fallback_observation() {
    let queued = reduce(
        &ready_state(),
        Event::KeyFallbackQueued(KeyFallbackQueuedEvent {
            observed_at: Millis::new(120),
            due_at: Millis::new(145),
        }),
    );
    let token = TimerToken::new(TimerId::Ingress, TimerGeneration::new(1));

    let activated = reduce(&queued.next, ingress_tick_event(token, 145));

    assert_eq!(activated.next.lifecycle(), Lifecycle::Observing);
    assert_eq!(
        activated.effects,
        vec![Effect::RequestObservationBase(
            RequestObservationBaseEffect {
                request: observation_request(1, ExternalDemandKind::KeyFallback, 145),
                context: observation_runtime_context(&queued.next),
            }
        )]
    );
}

#[test]
fn recent_autocmd_suppresses_deferred_key_fallback_when_ingress_timer_fires() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.delay_after_key = 25.0;
    let queued = reduce(
        &ready_state().with_runtime(runtime.clone()),
        Event::KeyFallbackQueued(KeyFallbackQueuedEvent {
            observed_at: Millis::new(120),
            due_at: Millis::new(145),
        }),
    );
    let token = TimerToken::new(TimerId::Ingress, TimerGeneration::new(1));
    let state = queued
        .next
        .with_runtime(runtime)
        .with_ingress_policy(IngressPolicyState::default().note_cursor_autocmd(Millis::new(130)));

    let suppressed = reduce(&state, ingress_tick_event(token, 145));

    assert_eq!(suppressed.next.lifecycle(), Lifecycle::Primed);
    assert!(suppressed.effects.is_empty());
}

#[test]
fn external_cursor_supersedes_pending_deferred_key_fallback() {
    let queued = reduce(
        &ready_state(),
        Event::KeyFallbackQueued(KeyFallbackQueuedEvent {
            observed_at: Millis::new(120),
            due_at: Millis::new(145),
        }),
    );

    let superseded = reduce(
        &queued.next,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(121),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    );

    assert_eq!(superseded.next.lifecycle(), Lifecycle::Observing);
    assert_eq!(
        superseded.effects,
        vec![
            Effect::RecordEventLoopMetric(EventLoopMetricEffect::IngressCoalesced),
            Effect::RequestObservationBase(RequestObservationBaseEffect {
                request: observation_request(2, ExternalDemandKind::ExternalCursor, 121),
                context: observation_runtime_context(&queued.next),
            }),
        ]
    );
}

#[test]
fn cursor_autocmd_demands_refresh_ingress_policy_state() {
    let transition = reduce(
        &ready_state(),
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ModeChanged,
            observed_at: Millis::new(77),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    );

    assert_eq!(
        transition.next.ingress_policy().last_cursor_autocmd_at(),
        Some(Millis::new(77))
    );
}

#[test]
fn cursor_ingress_emits_explicit_presentation_effect_before_observation_request() {
    let state = ready_state();
    let cell = ScreenCell::new(6, 9).expect("valid test cell");

    let transition = reduce(
        &state,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(78),
            requested_target: None,
            ingress_cursor_presentation: Some(IngressCursorPresentationRequest::new(
                true,
                true,
                Some(cell),
            )),
        }),
    );

    assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    assert_eq!(
        transition.effects,
        vec![
            Effect::ApplyIngressCursorPresentation(
                IngressCursorPresentationEffect::HideCursorAndPrepaint {
                    cell,
                    zindex: state.runtime().config.windows_zindex,
                },
            ),
            Effect::RequestObservationBase(RequestObservationBaseEffect {
                request: observation_request(1, ExternalDemandKind::ExternalCursor, 78),
                context: observation_runtime_context(&state),
            }),
        ]
    );
}

#[test]
fn animation_timer_from_ready_enters_planning_and_requests_render_plan() {
    let base = ready_state_with_observation(cursor(9, 9));
    let (state, token) = timer_armed_state(base);

    let transition = reduce(&state, animation_tick_event(token, 50));

    assert_eq!(transition.next.lifecycle(), Lifecycle::Planning);
    assert!(transition.next.pending_proposal().is_none());
    assert!(transition.next.pending_plan_proposal_id().is_some());
    assert!(matches!(
        transition.effects.as_slice(),
        [Effect::RequestRenderPlan(payload)] if payload.requested_at == Millis::new(50)
    ));
}

#[test]
fn animation_timer_uses_timer_timestamp_when_observation_clock_is_stale() {
    let request = observation_request(9, ExternalDemandKind::ExternalCursor, 100);
    let observation = ObservationSnapshot::new(
        request.clone(),
        observation_basis(&request, Some(cursor(9, 9)), 100),
        ProbeSet::default(),
        observation_motion(),
    );
    let mut runtime = ready_state().runtime().clone();
    runtime.initialize_cursor(
        Point { row: 9.0, col: 9.0 },
        CursorShape::new(false, false),
        7,
        CursorLocation::new(11, 22, 3, 4),
    );
    runtime.start_tail_drain(2);
    runtime.set_last_tick_ms(Some(100.0));
    let base = ready_state()
        .with_last_cursor(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .into_ready_with_observation(observation);
    let (state, token) = timer_armed_state(base);

    let transition = reduce(&state, animation_tick_event(token, 116));

    let [Effect::RequestRenderPlan(payload)] = transition.effects.as_slice() else {
        panic!("expected render plan request after stale-clock animation tick");
    };
    assert_eq!(transition.next.lifecycle(), Lifecycle::Planning);
    let planned_render = crate::core::reducer::build_planned_render(
        &payload.planning_state,
        payload.proposal_id,
        &payload.render_decision,
        payload.should_schedule_next_animation,
        payload.next_animation_at_ms,
    );
    let proposal = planned_render.proposal();
    let RealizationPlan::Draw(_) = proposal.realization() else {
        panic!(
            "expected draw proposal after timer-driven drain progress, got {:?}",
            proposal.realization()
        );
    };
    let next_animation_at = proposal
        .next_animation_at_ms()
        .expect("draw proposal should schedule another animation tick");
    assert!(
        next_animation_at.value() > 116,
        "next animation deadline should advance from timer time, got {}",
        next_animation_at.value()
    );
}

#[test]
fn animation_timer_draw_updates_scene_and_projection_cache() {
    let (state, _proposal_id) =
        planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), 61);

    let scene = state.scene();
    assert_eq!(scene.revision().value(), 1);
    assert_eq!(
        scene.dirty().entities(),
        &std::collections::BTreeSet::from([SemanticEntityId::CursorTrail])
    );

    let projection = scene
        .projection_entry()
        .expect("projection cache after draw render")
        .snapshot()
        .clone();
    assert_eq!(projection.witness().observation_id().value(), 9);
    assert_eq!(
        projection.witness().viewport(),
        ViewportSnapshot::new(CursorRow(40), CursorCol(120))
    );
    assert_eq!(
        projection
            .logical_raster()
            .clear()
            .map(|clear| clear.max_kept_windows),
        Some(state.runtime().config.max_kept_windows)
    );

    let Some(proposal) = state.pending_proposal() else {
        panic!("expected staged render proposal");
    };
    let RealizationPlan::Draw(draw) = proposal.realization() else {
        panic!("expected draw realization plan");
    };
    assert_eq!(
        scene
            .projection_entry()
            .expect("projection cache entry after draw render")
            .reuse_key()
            .target_cell_presentation(),
        proposal.side_effects().target_cell_presentation
    );
    assert_eq!(
        draw.palette().color_levels(),
        state.runtime().config.color_levels
    );
    assert_eq!(
        draw.max_kept_windows(),
        state.runtime().config.max_kept_windows
    );
    assert_eq!(proposal.patch().basis().target(), Some(&projection));
}

#[test]
fn apply_completed_advances_acknowledged_projection() {
    let (staged, proposal_id) =
        planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), 62);
    let acknowledged = staged
        .pending_proposal()
        .and_then(|proposal| proposal.patch().basis().target().cloned())
        .expect("target projection for apply completion");

    let completed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(63),
            visual_change: true,
        }),
    );

    assert_eq!(completed.next.lifecycle(), Lifecycle::Ready);
    assert_eq!(
        completed.next.realization(),
        &RealizationLedger::Consistent {
            acknowledged: Some(acknowledged),
        }
    );
}

#[test]
fn render_cleanup_applied_clears_trusted_realization_basis() {
    let (staged, proposal_id) =
        planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), 64);
    let acknowledged = staged
        .pending_proposal()
        .and_then(|proposal| proposal.patch().basis().target().cloned())
        .expect("target projection for cleanup");
    let ready = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(65),
            visual_change: true,
        }),
    )
    .next;

    let cleaned = reduce(
        &ready,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(66),
            action: RenderCleanupAppliedAction::SoftCleared,
        }),
    );

    assert_eq!(cleaned.next.lifecycle(), Lifecycle::Ready);
    assert_eq!(
        cleaned.next.realization(),
        &RealizationLedger::Diverged {
            last_consistent: Some(acknowledged.clone()),
            divergence: RealizationDivergence::ShellStateUnknown,
        }
    );
    assert_eq!(
        cleaned.next.realization().trusted_acknowledged_for_patch(),
        None
    );
    assert_eq!(
        cleaned.next.realization().last_consistent(),
        Some(&acknowledged)
    );
}

#[test]
fn untrusted_target_basis_derives_replace_patch() {
    let (staged, proposal_id) =
        planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), 67);
    let target = staged
        .pending_proposal()
        .and_then(|proposal| proposal.patch().basis().target().cloned())
        .expect("target projection for cleanup noop regression");
    let ready = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(68),
            visual_change: true,
        }),
    )
    .next;
    let cleaned = reduce(
        &ready,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(69),
            action: RenderCleanupAppliedAction::HardPurged,
        }),
    );

    let patch = ScenePatch::derive(PatchBasis::new(
        cleaned
            .next
            .realization()
            .trusted_acknowledged_for_patch()
            .cloned(),
        Some(target),
    ));

    assert_eq!(patch.kind(), crate::core::state::ScenePatchKind::Replace);
}

#[test]
fn apply_completion_emits_explicit_cleanup_and_redraw_effects() {
    let mut runtime = ready_state_with_observation(cursor(4, 9)).runtime().clone();
    runtime.config.max_kept_windows = 21;
    let state = ready_state_with_observation(cursor(4, 9)).with_runtime(runtime.clone());
    let basis = PatchBasis::new(None, None);
    let patch = ScenePatch::derive(basis.clone());
    let (state, proposal_id) = state.allocate_proposal_id();
    let proposal = InFlightProposal::new(
        proposal_id,
        basis,
        patch,
        RealizationPlan::Clear(RealizationClear::new(21)),
        RenderCleanupAction::Schedule,
        RenderSideEffects {
            redraw_after_clear_if_cmdline: true,
            ..RenderSideEffects::default()
        },
        false,
        None,
    );
    let staged = state
        .into_applying(proposal)
        .expect("staging clear proposal requires retained observation");

    let completed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(79),
            visual_change: true,
        }),
    );

    assert_eq!(completed.next.lifecycle(), Lifecycle::Ready);
    let cleanup_token = completed
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("cleanup timer should be armed");
    assert_eq!(
        completed.effects,
        vec![
            Effect::ScheduleTimer(ScheduleTimerEffect {
                kind: TimerKind::Cleanup,
                token: cleanup_token,
                delay: DelayBudgetMs::try_new(render_cleanup_delay_ms(&runtime.config))
                    .expect("cleanup delay budget"),
                requested_at: Millis::new(79),
            }),
            Effect::RedrawCmdline,
        ]
    );
}

#[test]
fn clear_apply_completion_redraws_after_visual_change_outside_cmdline() {
    let mut runtime = ready_state_with_observation(cursor(4, 9)).runtime().clone();
    runtime.config.max_kept_windows = 21;
    let state = ready_state_with_observation(cursor(4, 9)).with_runtime(runtime.clone());
    let basis = PatchBasis::new(None, None);
    let patch = ScenePatch::derive(basis.clone());
    let (state, proposal_id) = state.allocate_proposal_id();
    let proposal = InFlightProposal::new(
        proposal_id,
        basis,
        patch,
        RealizationPlan::Clear(RealizationClear::new(21)),
        RenderCleanupAction::Schedule,
        RenderSideEffects::default(),
        false,
        None,
    );
    let staged = state
        .into_applying(proposal)
        .expect("staging clear proposal requires retained observation");

    let completed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(79),
            visual_change: true,
        }),
    );

    let cleanup_token = completed
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("cleanup timer should be armed");
    assert_eq!(
        completed.effects,
        vec![
            Effect::ScheduleTimer(ScheduleTimerEffect {
                kind: TimerKind::Cleanup,
                token: cleanup_token,
                delay: DelayBudgetMs::try_new(render_cleanup_delay_ms(&runtime.config))
                    .expect("cleanup delay budget"),
                requested_at: Millis::new(79),
            }),
            Effect::RedrawCmdline,
        ]
    );
}

#[test]
fn cleanup_timer_soft_clear_rearms_hard_purge_without_new_ingress() {
    let mut runtime = ready_state_with_observation(cursor(4, 9)).runtime().clone();
    runtime.config.max_kept_windows = 21;
    let state = ready_state_with_observation(cursor(4, 9)).with_runtime(runtime.clone());
    let basis = PatchBasis::new(None, None);
    let patch = ScenePatch::derive(basis.clone());
    let (state, proposal_id) = state.allocate_proposal_id();
    let proposal = InFlightProposal::new(
        proposal_id,
        basis,
        patch,
        RealizationPlan::Clear(RealizationClear::new(21)),
        RenderCleanupAction::Schedule,
        RenderSideEffects::default(),
        false,
        None,
    );
    let staged = state
        .into_applying(proposal)
        .expect("staging clear proposal requires retained observation");

    let completed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(79),
            visual_change: true,
        }),
    );
    let soft_token = completed
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("soft cleanup timer should be armed");

    let soft_tick = reduce(
        &completed.next,
        cleanup_tick_event(soft_token, 79 + render_cleanup_delay_ms(&runtime.config)),
    );

    assert_eq!(
        soft_tick.effects,
        vec![Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
            execution: RenderCleanupExecution::SoftClear {
                max_kept_windows: 21,
            },
        })]
    );

    let after_soft = reduce(
        &soft_tick.next,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
            action: RenderCleanupAppliedAction::SoftCleared,
        }),
    );
    let hard_token = after_soft
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("hard purge timer should be armed after soft clear");

    let hard_tick = reduce(
        &after_soft.next,
        cleanup_tick_event(
            hard_token,
            79 + render_hard_cleanup_delay_ms(&runtime.config),
        ),
    );

    assert_eq!(
        hard_tick.effects,
        vec![Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
            execution: RenderCleanupExecution::HardPurge,
        })]
    );

    let after_hard = reduce(
        &hard_tick.next,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(79 + render_hard_cleanup_delay_ms(&runtime.config)),
            action: RenderCleanupAppliedAction::HardPurged,
        }),
    );

    assert_eq!(
        after_hard.next.timers().active_token(TimerId::Cleanup),
        None
    );
}

#[test]
fn diverged_realization_cannot_derive_noop_for_identical_target() {
    let (staged, proposal_id) =
        planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), 86);
    let target = staged
        .pending_proposal()
        .and_then(|proposal| proposal.patch().basis().target().cloned())
        .expect("target projection for divergence noop regression");
    let ready = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(87),
            visual_change: true,
        }),
    )
    .next;
    let diverged = ready.with_realization(RealizationLedger::diverged_from(
        Some(target.clone()),
        RealizationDivergence::ShellStateUnknown,
    ));

    let patch = ScenePatch::derive(PatchBasis::new(
        diverged
            .realization()
            .trusted_acknowledged_for_patch()
            .cloned(),
        Some(target),
    ));

    assert_eq!(patch.kind(), crate::core::state::ScenePatchKind::Replace);
}

#[test]
fn apply_completed_clears_proposal_and_resumes_pending_observation_before_animation() {
    let (staged, proposal_id) = applying_state_with_realization_plan(
        ready_state_with_observation(cursor(4, 9)),
        noop_realization_plan(),
        true,
        Some(Millis::new(90)),
    );
    let staged = reduce(
        &staged,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ModeChanged,
            observed_at: Millis::new(71),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
    )
    .next;

    let completed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(72),
            visual_change: false,
        }),
    );

    assert_eq!(completed.next.lifecycle(), Lifecycle::Observing);
    assert_eq!(completed.next.realization(), &RealizationLedger::Cleared);
    assert_eq!(completed.effects.len(), 2);
    assert_eq!(
        completed.effects[0],
        Effect::RequestObservationBase(RequestObservationBaseEffect {
            request: observation_request(1, ExternalDemandKind::ModeChanged, 71),
            context: observation_runtime_context(&staged),
        })
    );
    assert!(matches!(
        completed.effects[1],
        Effect::ScheduleTimer(ScheduleTimerEffect {
            kind: TimerKind::Animation,
            ..
        })
    ));
}

#[test]
fn full_apply_acknowledges_target_projection() {
    let (staged, proposal_id) =
        planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), 73);
    let expected = staged
        .scene()
        .projection_entry()
        .expect("projection cache after draw render")
        .snapshot()
        .clone();

    let completed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(74),
            visual_change: true,
        }),
    );

    assert_eq!(completed.next.lifecycle(), Lifecycle::Ready);
    assert_eq!(
        completed.next.realization(),
        &RealizationLedger::Consistent {
            acknowledged: Some(expected),
        }
    );
}

#[test]
fn failed_apply_preserves_last_acknowledged_basis_in_divergence() {
    let (seeded, _seed_proposal_id) =
        planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), 75);
    let acknowledged = seeded
        .scene()
        .projection_entry()
        .expect("projection cache after draw render")
        .snapshot()
        .clone();

    let state = ready_state_with_observation(cursor(4, 9)).with_realization(
        RealizationLedger::Consistent {
            acknowledged: Some(acknowledged.clone()),
        },
    );
    let (staged, proposal_id) =
        applying_state_with_realization_plan(state, noop_realization_plan(), false, None);

    let failed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::ApplyFailed {
            proposal_id,
            reason: crate::core::state::ApplyFailureKind::ShellError,
            divergence: RealizationDivergence::ShellStateUnknown,
            observed_at: Millis::new(77),
        }),
    );

    assert_eq!(failed.next.lifecycle(), Lifecycle::Recovering);
    assert_eq!(
        failed.next.realization(),
        &RealizationLedger::Diverged {
            last_consistent: Some(acknowledged),
            divergence: RealizationDivergence::ShellStateUnknown,
        }
    );
}

#[test]
fn viewport_drift_apply_failure_enters_recovering() {
    let (staged, proposal_id) = applying_state_with_realization_plan(
        ready_state_with_observation(cursor(4, 9)),
        noop_realization_plan(),
        false,
        None,
    );

    let failed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::ApplyFailed {
            proposal_id,
            reason: crate::core::state::ApplyFailureKind::ViewportDrift,
            divergence: RealizationDivergence::ShellStateUnknown,
            observed_at: Millis::new(78),
        }),
    );

    assert_eq!(failed.next.lifecycle(), Lifecycle::Recovering);
    assert_eq!(
        failed.next.realization(),
        &RealizationLedger::Diverged {
            last_consistent: None,
            divergence: RealizationDivergence::ShellStateUnknown,
        }
    );
}

#[test]
fn stale_apply_report_is_ignored_by_proposal_id() {
    let (staged, proposal_id) = applying_state_with_realization_plan(
        ready_state_with_observation(cursor(4, 9)),
        noop_realization_plan(),
        false,
        None,
    );

    let stale = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id: proposal_id.next(),
            observed_at: Millis::new(79),
            visual_change: true,
        }),
    );

    assert_eq!(stale.next, staged);
    assert_eq!(
        stale.effects,
        vec![Effect::RecordEventLoopMetric(
            EventLoopMetricEffect::StaleToken
        )]
    );
}

#[test]
fn degraded_apply_enters_recovering_and_schedules_recovery_timer() {
    let (staged, proposal_id) = applying_state_with_realization_plan(
        ready_state_with_observation(cursor(4, 9)),
        noop_realization_plan(),
        false,
        None,
    );

    let transition = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedDegraded {
            proposal_id,
            divergence: RealizationDivergence::ApplyMetrics(DegradedApplyMetrics::new(
                8, 5, 2, 1, 0, 0, 1,
            )),
            observed_at: Millis::new(81),
            visual_change: true,
        }),
    );

    assert_eq!(transition.next.lifecycle(), Lifecycle::Recovering);
    assert_eq!(
        transition.next.realization(),
        &RealizationLedger::Diverged {
            last_consistent: None,
            divergence: RealizationDivergence::ApplyMetrics(DegradedApplyMetrics::new(
                8, 5, 2, 1, 0, 0, 1,
            )),
        }
    );
    assert!(matches!(
        transition.effects.as_slice(),
        [Effect::ScheduleTimer(ScheduleTimerEffect {
            kind: TimerKind::Recovery,
            ..
        })]
    ));
}

#[test]
fn degraded_apply_keeps_last_acknowledged_projection() {
    let (first_staged, first_proposal_id) =
        planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), 82);
    let acknowledged = first_staged
        .pending_proposal()
        .and_then(|proposal| proposal.patch().basis().target().cloned())
        .expect("first acknowledged target");
    let ready = reduce(
        &first_staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id: first_proposal_id,
            observed_at: Millis::new(83),
            visual_change: true,
        }),
    )
    .next;

    let (staged, second_proposal_id) =
        applying_state_with_realization_plan(ready, noop_realization_plan(), false, None);

    let divergence =
        RealizationDivergence::ApplyMetrics(DegradedApplyMetrics::new(10, 7, 1, 1, 0, 0, 0));
    let degraded = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedDegraded {
            proposal_id: second_proposal_id,
            divergence,
            observed_at: Millis::new(85),
            visual_change: false,
        }),
    );

    assert_eq!(degraded.next.lifecycle(), Lifecycle::Recovering);
    assert_eq!(
        degraded.next.realization(),
        &RealizationLedger::Diverged {
            last_consistent: Some(acknowledged),
            divergence,
        }
    );
}

#[test]
fn effect_failure_is_ignored_before_initialize() {
    let state = CoreState::default();

    let transition = reduce(
        &state,
        Event::EffectFailed(EffectFailedEvent {
            proposal_id: None,
            observed_at: Millis::new(99),
        }),
    );

    assert_eq!(transition.next, state);
    assert!(transition.effects.is_empty());
}

#[test]
fn effect_failure_for_pending_proposal_preserves_acknowledged_basis_in_divergence() {
    let (first_staged, first_proposal_id) =
        planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), 82);
    let acknowledged = first_staged
        .pending_proposal()
        .and_then(|proposal| proposal.patch().basis().target().cloned())
        .expect("first acknowledged target");
    let ready = reduce(
        &first_staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id: first_proposal_id,
            observed_at: Millis::new(83),
            visual_change: true,
        }),
    )
    .next;

    let (staged, proposal_id) =
        applying_state_with_realization_plan(ready, noop_realization_plan(), false, None);

    let failed = reduce(
        &staged,
        Event::EffectFailed(EffectFailedEvent {
            proposal_id: Some(proposal_id),
            observed_at: Millis::new(85),
        }),
    );

    assert_eq!(failed.next.lifecycle(), Lifecycle::Recovering);
    assert_eq!(
        failed.next.realization(),
        &RealizationLedger::Diverged {
            last_consistent: Some(acknowledged),
            divergence: RealizationDivergence::ShellStateUnknown,
        }
    );
    assert!(matches!(
        failed.effects.as_slice(),
        [Effect::ScheduleTimer(ScheduleTimerEffect {
            kind: TimerKind::Recovery,
            ..
        })]
    ));
}

#[test]
fn stale_effect_failure_is_ignored_by_proposal_id() {
    let (staged, proposal_id) = applying_state_with_realization_plan(
        ready_state_with_observation(cursor(4, 7)),
        noop_realization_plan(),
        false,
        None,
    );
    let stale_proposal_id = proposal_id.next();

    let stale = reduce(
        &staged,
        Event::EffectFailed(EffectFailedEvent {
            proposal_id: Some(stale_proposal_id),
            observed_at: Millis::new(86),
        }),
    );

    assert_eq!(stale.next, staged);
    assert_eq!(
        stale.effects,
        vec![Effect::RecordEventLoopMetric(
            EventLoopMetricEffect::StaleToken
        )]
    );
}

#[test]
fn stale_timer_token_is_ignored_without_mutating_state() {
    let state = recovering_state_with_observation(cursor(2, 2));
    let (timers, stale_token) = state.timers().arm(TimerId::Recovery);
    let state = state.with_timers(timers);
    let (timers, _fresh_token) = state.timers().arm(TimerId::Recovery);
    let state = state.with_timers(timers);

    let transition = reduce(
        &state,
        Event::TimerLostWithToken(TimerLostWithTokenEvent {
            kind: TimerKind::Recovery,
            token: stale_token,
            observed_at: Millis::new(120),
        }),
    );

    assert_eq!(transition.next, state);
    assert_eq!(
        transition.effects,
        vec![Effect::RecordEventLoopMetric(
            EventLoopMetricEffect::StaleToken
        )]
    );
}

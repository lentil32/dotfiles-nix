use super::Transition;
use super::reduce;
use crate::core::effect::ApplyRenderCleanupEffect;
use crate::core::effect::CursorPositionReadPolicy;
use crate::core::effect::Effect;
use crate::core::effect::EventLoopMetricEffect;
use crate::core::effect::IngressCursorPresentationEffect;
use crate::core::effect::IngressCursorPresentationRequest;
use crate::core::effect::ObservationRuntimeContext;
use crate::core::effect::ProbePolicy;
use crate::core::effect::ProbeQuality;
use crate::core::effect::RenderCleanupExecution;
use crate::core::effect::RequestObservationBaseEffect;
use crate::core::effect::RequestProbeEffect;
use crate::core::effect::ScheduleTimerEffect;
use crate::core::event::ApplyReport;
use crate::core::event::EffectFailedEvent;
use crate::core::event::Event;
use crate::core::event::ExternalDemandQueuedEvent;
use crate::core::event::InitializeEvent;
use crate::core::event::ObservationBaseCollectedEvent;
use crate::core::event::ProbeReportedEvent;
use crate::core::event::RenderCleanupAppliedAction;
use crate::core::event::RenderCleanupAppliedEvent;
use crate::core::event::RenderPlanComputedEvent;
use crate::core::event::TimerFiredWithTokenEvent;
use crate::core::event::TimerLostWithTokenEvent;
use crate::core::runtime_reducer::RenderAction;
use crate::core::runtime_reducer::RenderCleanupAction;
use crate::core::runtime_reducer::RenderSideEffects;
use crate::core::runtime_reducer::render_cleanup_delay_ms;
use crate::core::runtime_reducer::render_hard_cleanup_delay_ms;
use crate::core::state::BackgroundProbeBatch;
use crate::core::state::BackgroundProbeChunk;
use crate::core::state::BackgroundProbeChunkMask;
use crate::core::state::BackgroundProbePlan;
use crate::core::state::BufferPerfClass;
use crate::core::state::CoreState;
use crate::core::state::CursorColorSample;
use crate::core::state::CursorPositionSync;
use crate::core::state::CursorTextContext;
use crate::core::state::DegradedApplyMetrics;
use crate::core::state::DemandQueue;
use crate::core::state::ExternalDemand;
use crate::core::state::ExternalDemandKind;
use crate::core::state::InFlightProposal;
use crate::core::state::ObservationBasis;
use crate::core::state::ObservationMotion;
use crate::core::state::ObservationRequest;
use crate::core::state::ObservationSnapshot;
use crate::core::state::ObservedTextRow;
use crate::core::state::PatchBasis;
use crate::core::state::ProbeFailure;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeRequestSet;
use crate::core::state::ProbeReuse;
use crate::core::state::ProbeSlot;
use crate::core::state::ProbeState;
use crate::core::state::RealizationClear;
use crate::core::state::RealizationDivergence;
use crate::core::state::RealizationLedger;
use crate::core::state::RealizationPlan;
use crate::core::state::RecoveryPolicyState;
use crate::core::state::RenderThermalState;
use crate::core::state::ScenePatch;
use crate::core::state::SemanticEntityId;
use crate::core::types::CursorCol;
use crate::core::types::CursorPosition;
use crate::core::types::CursorRow;
use crate::core::types::DelayBudgetMs;
use crate::core::types::IngressSeq;
use crate::core::types::Lifecycle;
use crate::core::types::Millis;
use crate::core::types::ProposalId;
use crate::core::types::TimerId;
use crate::core::types::TimerToken;
use crate::core::types::ViewportSnapshot;
use crate::state::CursorLocation;
use crate::state::CursorShape;
use crate::state::RuntimeState;
use crate::types::Point;
use crate::types::ScreenCell;
use pretty_assertions::assert_eq as pretty_assert_eq;

fn cursor(row: u32, col: u32) -> CursorPosition {
    CursorPosition {
        row: CursorRow(row),
        col: CursorCol(col),
    }
}

fn noop_realization_plan() -> RealizationPlan {
    RealizationPlan::Noop
}

fn with_cleanup_invalidation(
    next_state: &CoreState,
    observed_at: u64,
    mut effects: Vec<Effect>,
) -> Vec<Effect> {
    let cleanup_token = next_state
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("cleanup timer should be armed");
    effects.push(Effect::ScheduleTimer(ScheduleTimerEffect {
        token: cleanup_token,
        delay: DelayBudgetMs::try_new(render_cleanup_delay_ms(&next_state.runtime().config))
            .expect("cleanup delay budget"),
        requested_at: Millis::new(observed_at),
    }));
    effects
}

fn ready_state() -> CoreState {
    let mut runtime = RuntimeState::default();
    runtime.config.delay_event_to_smear = 0.0;
    CoreState::default().with_runtime(runtime).initialize()
}

fn ready_state_with_runtime_config(configure: impl FnOnce(&mut RuntimeState)) -> CoreState {
    let ready = ready_state();
    let mut runtime = ready.runtime().clone();
    configure(&mut runtime);
    ready.with_runtime(runtime)
}

fn external_demand_event(
    kind: ExternalDemandKind,
    observed_at: u64,
    requested_target: Option<CursorPosition>,
) -> Event {
    external_demand_event_with_perf_class(
        kind,
        observed_at,
        requested_target,
        BufferPerfClass::Full,
    )
}

fn external_demand_event_with_perf_class(
    kind: ExternalDemandKind,
    observed_at: u64,
    requested_target: Option<CursorPosition>,
    buffer_perf_class: BufferPerfClass,
) -> Event {
    Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
        kind,
        observed_at: Millis::new(observed_at),
        requested_target,
        buffer_perf_class,
        ingress_cursor_presentation: None,
    })
}

fn observation_request(seq: u64, kind: ExternalDemandKind, observed_at: u64) -> ObservationRequest {
    observation_request_with_perf_class(seq, kind, observed_at, BufferPerfClass::Full)
}

fn observation_request_with_perf_class(
    seq: u64,
    kind: ExternalDemandKind,
    observed_at: u64,
    buffer_perf_class: BufferPerfClass,
) -> ObservationRequest {
    ObservationRequest::new(
        ExternalDemand::new(
            IngressSeq::new(seq),
            kind,
            Millis::new(observed_at),
            None,
            buffer_perf_class,
        ),
        ProbeRequestSet::default(),
    )
}

fn observation_basis(
    request: &ObservationRequest,
    position: Option<CursorPosition>,
    observed_at: u64,
) -> ObservationBasis {
    observation_basis_in_mode(request, position, observed_at, "n")
}

fn observation_basis_in_mode(
    request: &ObservationRequest,
    position: Option<CursorPosition>,
    observed_at: u64,
    mode: &str,
) -> ObservationBasis {
    ObservationBasis::new(
        request.observation_id(),
        Millis::new(observed_at),
        mode.to_string(),
        position,
        CursorLocation::new(11, 22, 3, 4),
        ViewportSnapshot::new(CursorRow(40), CursorCol(120)),
    )
}

fn observed_rows(rows: &[(i64, &str)]) -> Vec<ObservedTextRow> {
    rows.iter()
        .map(|(line, text)| ObservedTextRow::new(*line, (*text).to_string()))
        .collect()
}

fn text_context(
    changedtick: u64,
    cursor_line: i64,
    rows: &[(i64, &str)],
    tracked_cursor_line: Option<i64>,
    tracked_rows: Option<&[(i64, &str)]>,
) -> CursorTextContext {
    CursorTextContext::new(
        22,
        changedtick,
        cursor_line,
        observed_rows(rows),
        tracked_cursor_line,
        tracked_rows.map(observed_rows),
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "The test fixture mirrors the distinct text-context inputs under comparison."
)]
fn observation_basis_with_text_context(
    request: &ObservationRequest,
    position: Option<CursorPosition>,
    observed_at: u64,
    cursor_line: i64,
    changedtick: u64,
    rows: &[(i64, &str)],
    tracked_cursor_line: Option<i64>,
    tracked_rows: Option<&[(i64, &str)]>,
) -> ObservationBasis {
    ObservationBasis::new(
        request.observation_id(),
        Millis::new(observed_at),
        "n".to_string(),
        position,
        CursorLocation::new(11, 22, 3, cursor_line),
        ViewportSnapshot::new(CursorRow(40), CursorCol(120)),
    )
    .with_cursor_text_context(Some(text_context(
        changedtick,
        cursor_line,
        rows,
        tracked_cursor_line,
        tracked_rows,
    )))
}

fn observation_motion() -> ObservationMotion {
    ObservationMotion::default()
}

fn observation_snapshot(position: CursorPosition) -> ObservationSnapshot {
    let request = observation_request(9, ExternalDemandKind::ExternalCursor, 90);
    let basis = observation_basis(&request, Some(position), 91);
    ObservationSnapshot::new(request, basis, observation_motion())
}

fn observation_snapshot_with_cursor_color(
    position: CursorPosition,
    color: u32,
) -> ObservationSnapshot {
    observation_snapshot_with_cursor_color_reuse(position, color, ProbeReuse::Exact)
}

fn observation_snapshot_with_cursor_color_reuse(
    position: CursorPosition,
    color: u32,
    reuse: ProbeReuse,
) -> ObservationSnapshot {
    let request = ObservationRequest::new(
        ExternalDemand::new(
            IngressSeq::new(9),
            ExternalDemandKind::ExternalCursor,
            Millis::new(90),
            None,
            BufferPerfClass::Full,
        ),
        ProbeRequestSet::new(true, false),
    );
    let basis = observation_basis(&request, Some(position), 91);
    ObservationSnapshot::new(request.clone(), basis, observation_motion())
        .with_cursor_color_probe(ProbeState::ready(
            ProbeKind::CursorColor.request_id(request.observation_id()),
            request.observation_id(),
            reuse,
            Some(CursorColorSample::new(color)),
        ))
        .expect("cursor color probe should be requested")
}

fn observing_state_from_demand(
    ready: &CoreState,
    kind: ExternalDemandKind,
    observed_at: u64,
    requested_target: Option<CursorPosition>,
) -> CoreState {
    reduce(
        ready,
        external_demand_event(kind, observed_at, requested_target),
    )
    .next
}

fn active_request(state: &CoreState) -> ObservationRequest {
    state
        .active_observation_request()
        .cloned()
        .expect("active observation request")
}

fn collect_observation_base(
    state: &CoreState,
    request: &ObservationRequest,
    basis: ObservationBasis,
    motion: ObservationMotion,
) -> Transition {
    reduce(
        state,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis,
            motion,
        }),
    )
}

fn cursor_color_probe_ready_state() -> CoreState {
    ready_state_with_runtime_config(|runtime| {
        runtime.config.cursor_color = Some("none".to_string());
    })
}

fn background_probe_ready_state() -> CoreState {
    ready_state_with_runtime_config(|runtime| {
        runtime.config.particles_enabled = true;
        runtime.config.particles_over_text = false;
    })
}

fn dual_probe_ready_state() -> CoreState {
    ready_state_with_runtime_config(|runtime| {
        runtime.config.cursor_color = Some("none".to_string());
        runtime.config.particles_enabled = true;
        runtime.config.particles_over_text = false;
    })
}

struct ObservationScenario {
    observing: CoreState,
    request: ObservationRequest,
    basis: ObservationBasis,
    based: Transition,
}

fn sparse_probe_cells(viewport: ViewportSnapshot, count: usize) -> Vec<ScreenCell> {
    let width = i64::from(viewport.max_col.value());
    (0..count)
        .map(|index| {
            let index = i64::try_from(index).expect("probe cell index");
            let row = index / width + 1;
            let col = index % width + 1;
            ScreenCell::new(row, col).expect("probe cell")
        })
        .collect()
}

impl ObservationScenario {
    fn new(ready: CoreState) -> Self {
        let observing =
            observing_state_from_demand(&ready, ExternalDemandKind::ExternalCursor, 25, None);
        let request = active_request(&observing);
        let basis = observation_basis(&request, Some(cursor(7, 8)), 26);
        let based =
            collect_observation_base(&observing, &request, basis.clone(), observation_motion());
        Self {
            observing,
            request,
            basis,
            based,
        }
    }

    fn with_background_plan(ready: CoreState, plan_cells: Vec<ScreenCell>) -> Self {
        let observing =
            observing_state_from_demand(&ready, ExternalDemandKind::ExternalCursor, 25, None);
        let request = active_request(&observing);
        let basis = observation_basis(&request, Some(cursor(7, 8)), 26);
        let observation =
            ObservationSnapshot::new(request.clone(), basis.clone(), observation_motion())
                .with_background_probe_plan(BackgroundProbePlan::from_cells(plan_cells));
        let next = observing
            .clone()
            .with_last_cursor(Some(cursor(7, 8)))
            .with_active_observation(Some(observation.clone()))
            .expect("observation should stay active");
        let cursor_color_fallback_sample = retained_cursor_color_sample(&observing);
        let effect = if ready.runtime().config.requires_cursor_color_sampling() {
            Effect::RequestProbe(RequestProbeEffect {
                observation_basis: basis.clone(),
                probe_request_id: ProbeKind::CursorColor.request_id(request.observation_id()),
                kind: ProbeKind::CursorColor,
                cursor_position_policy: cursor_position_policy(&observing),
                buffer_perf_class: request.demand().buffer_perf_class(),
                probe_policy: expected_probe_policy(
                    request.demand().kind(),
                    request.demand().buffer_perf_class(),
                    cursor_color_fallback_sample,
                ),
                background_chunk: None,
                cursor_color_fallback_sample,
            })
        } else {
            Effect::RequestProbe(RequestProbeEffect {
                observation_basis: basis.clone(),
                probe_request_id: ProbeKind::Background.request_id(request.observation_id()),
                kind: ProbeKind::Background,
                cursor_position_policy: cursor_position_policy(&observing),
                buffer_perf_class: request.demand().buffer_perf_class(),
                probe_policy: expected_probe_policy(
                    request.demand().kind(),
                    request.demand().buffer_perf_class(),
                    cursor_color_fallback_sample,
                ),
                background_chunk: observation
                    .background_progress()
                    .and_then(crate::core::state::BackgroundProbeProgress::next_chunk),
                cursor_color_fallback_sample: None,
            })
        };

        Self {
            observing,
            request,
            basis,
            based: Transition::new(next, vec![effect]),
        }
    }

    fn with_background_probe_cell_count(ready: CoreState, cell_count: usize) -> Self {
        let observing =
            observing_state_from_demand(&ready, ExternalDemandKind::ExternalCursor, 25, None);
        let request = active_request(&observing);
        let basis = observation_basis(&request, Some(cursor(7, 8)), 26);
        Self::with_background_plan(ready, sparse_probe_cells(basis.viewport(), cell_count))
    }
}

fn cursor_position_policy(state: &CoreState) -> CursorPositionReadPolicy {
    CursorPositionReadPolicy::new(state.runtime().config.smear_to_cmd)
}

fn retained_cursor_color_sample(state: &CoreState) -> Option<CursorColorSample> {
    state
        .retained_observation()
        .and_then(ObservationSnapshot::cursor_color)
        .map(CursorColorSample::new)
}

fn expected_probe_policy(
    demand_kind: ExternalDemandKind,
    buffer_perf_class: BufferPerfClass,
    cursor_color_fallback_sample: Option<CursorColorSample>,
) -> ProbePolicy {
    ProbePolicy::for_demand(
        demand_kind,
        buffer_perf_class,
        cursor_color_fallback_sample.is_some(),
    )
}

fn compatible_cursor_color_ready_state(
    configure_runtime: impl FnOnce(&mut RuntimeState),
) -> CoreState {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.cursor_color = Some("none".to_string());
    runtime.initialize_cursor(
        Point { row: 9.0, col: 9.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(11, 22, 3, 9),
    );
    configure_runtime(&mut runtime);
    ready_state()
        .with_last_cursor(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .into_ready_with_observation(observation_snapshot_with_cursor_color_reuse(
            cursor(9, 9),
            0x00AB_CDEF,
            ProbeReuse::Compatible,
        ))
}

fn conceal_deferred_cursor_ready_state(
    configure_runtime: impl FnOnce(&mut RuntimeState),
) -> CoreState {
    let mut runtime = ready_state().runtime().clone();
    runtime.initialize_cursor(
        Point { row: 9.0, col: 9.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(11, 22, 3, 9),
    );
    configure_runtime(&mut runtime);

    let request = observation_request_with_perf_class(
        9,
        ExternalDemandKind::ExternalCursor,
        90,
        BufferPerfClass::FastMotion,
    );
    let basis = observation_basis(&request, Some(cursor(9, 9)), 91);
    let observation = ObservationSnapshot::new(
        request,
        basis,
        observation_motion().with_cursor_position_sync(CursorPositionSync::ConcealDeferred),
    );

    ready_state()
        .with_last_cursor(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .into_ready_with_observation(observation)
}

fn observation_runtime_context(
    state: &CoreState,
    demand_kind: ExternalDemandKind,
) -> ObservationRuntimeContext {
    observation_runtime_context_with_perf_class(state, demand_kind, BufferPerfClass::Full)
}

fn observation_runtime_context_with_perf_class(
    state: &CoreState,
    demand_kind: ExternalDemandKind,
    buffer_perf_class: BufferPerfClass,
) -> ObservationRuntimeContext {
    let cursor_color_fallback_sample = retained_cursor_color_sample(state);
    ObservationRuntimeContext::new(
        cursor_position_policy(state),
        state.runtime().config.scroll_buffer_space,
        state.runtime().tracked_location(),
        state.runtime().current_corners(),
        buffer_perf_class,
        expected_probe_policy(demand_kind, buffer_perf_class, cursor_color_fallback_sample),
    )
}

fn cursor_color_probe_report(
    request: &ObservationRequest,
    reuse: ProbeReuse,
    color: Option<u32>,
) -> Event {
    Event::ProbeReported(ProbeReportedEvent::CursorColorReady {
        observation_id: request.observation_id(),
        probe_request_id: ProbeKind::CursorColor.request_id(request.observation_id()),
        reuse,
        sample: color.map(CursorColorSample::new),
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
    chunk: &BackgroundProbeChunk,
    _viewport: ViewportSnapshot,
    allowed_cells: &[(u32, u32)],
) -> Event {
    let allowed_mask = chunk
        .cells()
        .iter()
        .map(|cell| {
            let Ok(row) = u32::try_from(cell.row()) else {
                return false;
            };
            let Ok(col) = u32::try_from(cell.col()) else {
                return false;
            };
            allowed_cells.contains(&(row, col))
        })
        .collect::<Vec<_>>();

    Event::ProbeReported(ProbeReportedEvent::BackgroundChunkReady {
        observation_id: request.observation_id(),
        probe_request_id: ProbeKind::Background.request_id(request.observation_id()),
        chunk: chunk.clone(),
        allowed_mask: BackgroundProbeChunkMask::from_allowed_mask(&allowed_mask),
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
        token,
        observed_at: Millis::new(observed_at),
    })
}

fn cleanup_tick_event(token: TimerToken, observed_at: u64) -> Event {
    Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
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
            planned_render: Box::new(
                crate::core::reducer::build_planned_render(
                    &payload.planning_state,
                    payload.proposal_id,
                    &payload.render_decision,
                    payload.animation_schedule,
                )
                .expect("planned render should satisfy proposal shape invariants"),
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
    let acknowledged = state
        .realization()
        .trusted_acknowledged_for_patch()
        .cloned();
    let target = match &realization {
        RealizationPlan::Draw(_) => acknowledged.clone().or_else(|| {
            state
                .scene()
                .projection_entry()
                .map(|entry| entry.snapshot().clone())
        }),
        RealizationPlan::Noop => acknowledged.clone(),
        RealizationPlan::Clear(_) | RealizationPlan::Failure(_) => None,
    };
    let basis = PatchBasis::new(acknowledged, target);
    let patch = ScenePatch::derive(basis);
    let (state, proposal_id) = state.allocate_proposal_id();
    let proposal = match realization {
        RealizationPlan::Draw(draw) => InFlightProposal::draw(
            proposal_id,
            patch,
            draw,
            RenderCleanupAction::NoAction,
            RenderSideEffects::default(),
            crate::core::state::AnimationSchedule::from_parts(
                should_schedule_next_animation,
                next_animation_at_ms,
            ),
        )
        .expect("test draw proposal should be constructible"),
        RealizationPlan::Clear(clear) => InFlightProposal::clear(
            proposal_id,
            patch,
            clear,
            RenderCleanupAction::NoAction,
            RenderSideEffects::default(),
            crate::core::state::AnimationSchedule::from_parts(
                should_schedule_next_animation,
                next_animation_at_ms,
            ),
        )
        .expect("test clear proposal should be constructible"),
        RealizationPlan::Noop => InFlightProposal::noop(
            proposal_id,
            patch,
            RenderCleanupAction::NoAction,
            RenderSideEffects::default(),
            crate::core::state::AnimationSchedule::from_parts(
                should_schedule_next_animation,
                next_animation_at_ms,
            ),
        )
        .expect("test noop proposal should be constructible"),
        RealizationPlan::Failure(failure) => InFlightProposal::failure(
            proposal_id,
            patch,
            failure,
            RenderCleanupAction::NoAction,
            RenderSideEffects::default(),
            crate::core::state::AnimationSchedule::from_parts(
                should_schedule_next_animation,
                next_animation_at_ms,
            ),
        ),
    };
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

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Primed);
    assert!(transition.effects.is_empty());
}

mod protocol_shared_state_constructors {
    use super::*;
    use crate::core::state::IngressPolicyState;

    fn primed_state_with_shared_policy() -> (
        CoreState,
        RecoveryPolicyState,
        IngressPolicyState,
        TimerToken,
    ) {
        let recovery_policy = RecoveryPolicyState::default().with_retry_attempt(3);
        let ingress_policy = IngressPolicyState::default().note_cursor_autocmd(Millis::new(55));
        let (timers, armed_token) = CoreState::default().timers().arm(TimerId::Animation);
        let primed = CoreState::default()
            .with_timers(timers)
            .with_recovery_policy(recovery_policy)
            .with_ingress_policy(ingress_policy)
            .initialize();
        (primed, recovery_policy, ingress_policy, armed_token)
    }

    fn assert_shared_protocol_state(
        state: &CoreState,
        expected_token: TimerToken,
        recovery_policy: RecoveryPolicyState,
        ingress_policy: IngressPolicyState,
    ) {
        pretty_assert_eq!(
            state.timers().active_token(TimerId::Animation),
            Some(expected_token)
        );
        pretty_assert_eq!(state.recovery_policy(), recovery_policy);
        pretty_assert_eq!(state.ingress_policy(), ingress_policy);
    }

    #[test]
    fn initialize_keeps_armed_timers_and_policy_state() {
        let (primed, recovery_policy, ingress_policy, armed_token) =
            primed_state_with_shared_policy();

        assert_shared_protocol_state(&primed, armed_token, recovery_policy, ingress_policy);
    }

    #[test]
    fn into_observing_keeps_armed_timers_and_policy_state() {
        let (primed, recovery_policy, ingress_policy, armed_token) =
            primed_state_with_shared_policy();
        let observing = primed
            .with_demand_queue(DemandQueue::default())
            .into_observing(observation_request(
                11,
                ExternalDemandKind::ExternalCursor,
                77,
            ));

        assert_shared_protocol_state(&observing, armed_token, recovery_policy, ingress_policy);
    }

    #[test]
    fn into_ready_with_observation_keeps_armed_timers_and_policy_state() {
        let (primed, recovery_policy, ingress_policy, armed_token) =
            primed_state_with_shared_policy();
        let observation = observation_snapshot(cursor(4, 9));
        let ready = primed
            .with_demand_queue(DemandQueue::default())
            .into_observing(observation_request(
                11,
                ExternalDemandKind::ExternalCursor,
                77,
            ))
            .with_active_observation(Some(observation.clone()))
            .expect("observation staging should succeed")
            .into_ready_with_observation(observation);

        assert_shared_protocol_state(&ready, armed_token, recovery_policy, ingress_policy);
    }

    #[test]
    fn into_recovering_keeps_armed_timers_and_policy_state() {
        let (primed, recovery_policy, ingress_policy, armed_token) =
            primed_state_with_shared_policy();
        let observation = observation_snapshot(cursor(4, 9));
        let ready = primed
            .with_demand_queue(DemandQueue::default())
            .into_observing(observation_request(
                11,
                ExternalDemandKind::ExternalCursor,
                77,
            ))
            .with_active_observation(Some(observation.clone()))
            .expect("observation staging should succeed")
            .into_ready_with_observation(observation);
        let recovering = ready.into_recovering();

        assert_shared_protocol_state(&recovering, armed_token, recovery_policy, ingress_policy);
    }

    #[test]
    fn clear_pending_for_keeps_armed_timers_and_policy_state() {
        let (primed, recovery_policy, ingress_policy, armed_token) =
            primed_state_with_shared_policy();
        let observation = observation_snapshot(cursor(4, 9));
        let ready = primed
            .with_demand_queue(DemandQueue::default())
            .into_observing(observation_request(
                11,
                ExternalDemandKind::ExternalCursor,
                77,
            ))
            .with_active_observation(Some(observation.clone()))
            .expect("observation staging should succeed")
            .into_ready_with_observation(observation);
        let (applying, proposal_id) =
            applying_state_with_realization_plan(ready, noop_realization_plan(), false, None);
        let (cleared, _) = applying
            .clear_pending_for(proposal_id)
            .expect("proposal should clear back to ready");

        assert_shared_protocol_state(&cleared, armed_token, recovery_policy, ingress_policy);
    }
}

mod observing_cursor_demand_queue {
    use super::*;

    #[test]
    fn first_cursor_demand_enters_observing_and_requests_observation_base() {
        let ready = ready_state();

        let first = reduce(
            &ready,
            external_demand_event(ExternalDemandKind::ExternalCursor, 20, None),
        );

        pretty_assert_eq!(first.next.lifecycle(), Lifecycle::Observing);
        pretty_assert_eq!(
            first.effects,
            with_cleanup_invalidation(
                &first.next,
                20,
                vec![Effect::RequestObservationBase(
                    RequestObservationBaseEffect {
                        request: observation_request(1, ExternalDemandKind::ExternalCursor, 20),
                        context: observation_runtime_context(
                            &ready,
                            ExternalDemandKind::ExternalCursor,
                        ),
                    }
                )]
            )
        );
    }

    #[test]
    fn second_cursor_demand_records_ingress_coalesced_without_restarting_observation() {
        let ready = ready_state();
        let observing =
            observing_state_from_demand(&ready, ExternalDemandKind::ExternalCursor, 20, None);

        let second = reduce(
            &observing,
            external_demand_event(ExternalDemandKind::ExternalCursor, 21, None),
        );

        pretty_assert_eq!(second.next.lifecycle(), Lifecycle::Observing);
        pretty_assert_eq!(
            second.effects,
            vec![Effect::RecordEventLoopMetric(
                EventLoopMetricEffect::IngressCoalesced,
            )]
        );
    }

    #[test]
    fn newest_queued_cursor_replaces_the_older_pending_cursor_demand() {
        let ready = ready_state();
        let observing =
            observing_state_from_demand(&ready, ExternalDemandKind::ExternalCursor, 20, None);
        let coalesced = reduce(
            &observing,
            external_demand_event(ExternalDemandKind::ExternalCursor, 21, None),
        );

        let third = reduce(
            &coalesced.next,
            external_demand_event(ExternalDemandKind::ExternalCursor, 22, None),
        );

        let queued_cursor = third
            .next
            .demand_queue()
            .latest_cursor()
            .expect("queued cursor demand");
        pretty_assert_eq!(
            queued_cursor,
            &crate::core::state::QueuedDemand::ready(ExternalDemand::new(
                IngressSeq::new(3),
                ExternalDemandKind::ExternalCursor,
                Millis::new(22),
                None,
                BufferPerfClass::Full,
            ))
        );
    }
}

mod delayed_cursor_demand_queue {
    use super::*;

    fn delayed_ready_state() -> CoreState {
        ready_state_with_runtime_config(|runtime| {
            runtime.config.delay_event_to_smear = 40.0;
        })
    }

    #[test]
    fn first_cursor_demand_arms_the_ingress_timer_before_observing() {
        let ready = delayed_ready_state();

        let first = reduce(
            &ready,
            external_demand_event(ExternalDemandKind::ExternalCursor, 20, None),
        );

        let ingress_token = first
            .next
            .timers()
            .active_token(TimerId::Ingress)
            .expect("cursor ingress delay should arm the ingress timer");
        pretty_assert_eq!(first.next.lifecycle(), ready.lifecycle());
        pretty_assert_eq!(
            first.effects,
            with_cleanup_invalidation(
                &first.next,
                20,
                vec![Effect::ScheduleTimer(ScheduleTimerEffect {
                    token: ingress_token,
                    delay: DelayBudgetMs::try_new(40).expect("ingress delay budget"),
                    requested_at: Millis::new(20),
                })],
            )
        );
        assert!(first.next.demand_queue().latest_cursor().is_some());
        pretty_assert_eq!(
            first.next.ingress_policy().pending_delay_until(),
            Some(Millis::new(60))
        );
    }

    #[test]
    fn delayed_cursor_timer_fire_starts_the_queued_observation() {
        let ready = delayed_ready_state();
        let delayed = reduce(
            &ready,
            external_demand_event(ExternalDemandKind::ExternalCursor, 20, None),
        )
        .next;
        let ingress_token = delayed
            .timers()
            .active_token(TimerId::Ingress)
            .expect("ingress timer token");

        let fired = reduce(
            &delayed,
            Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
                token: ingress_token,
                observed_at: Millis::new(60),
            }),
        );

        pretty_assert_eq!(fired.next.lifecycle(), Lifecycle::Observing);
        assert!(matches!(
            fired.effects.as_slice(),
            [Effect::RequestObservationBase(
                RequestObservationBaseEffect { .. }
            )]
        ));
    }

    #[test]
    fn newer_delayed_cursor_demand_replaces_the_pending_queue_without_rearming_the_timer() {
        let ready = delayed_ready_state();
        let first = reduce(
            &ready,
            external_demand_event(ExternalDemandKind::ExternalCursor, 20, None),
        );
        let first_token = first
            .next
            .timers()
            .active_token(TimerId::Ingress)
            .expect("first ingress timer token");

        let second = reduce(
            &first.next,
            external_demand_event(ExternalDemandKind::ExternalCursor, 21, None),
        );

        let second_token = second
            .next
            .timers()
            .active_token(TimerId::Ingress)
            .expect("existing ingress timer token");
        pretty_assert_eq!(second_token, first_token);
        pretty_assert_eq!(
            second.next.demand_queue().latest_cursor(),
            Some(&crate::core::state::QueuedDemand::ready(
                ExternalDemand::new(
                    IngressSeq::new(2),
                    ExternalDemandKind::ExternalCursor,
                    Millis::new(21),
                    None,
                    BufferPerfClass::Full,
                )
            ))
        );
        pretty_assert_eq!(
            second.effects,
            vec![
                Effect::RecordEventLoopMetric(EventLoopMetricEffect::IngressCoalesced),
                Effect::RecordEventLoopMetric(EventLoopMetricEffect::DelayedIngressPendingUpdated,),
            ]
        );
        pretty_assert_eq!(
            second.next.ingress_policy().pending_delay_until(),
            Some(Millis::new(61))
        );
    }

    #[test]
    fn delayed_cursor_burst_updates_pending_deadline_without_stale_timer_token_churn() {
        let ready = delayed_ready_state();
        let first = reduce(
            &ready,
            external_demand_event(ExternalDemandKind::ExternalCursor, 20, None),
        );
        let first_token = first
            .next
            .timers()
            .active_token(TimerId::Ingress)
            .expect("first ingress timer token");

        let second = reduce(
            &first.next,
            external_demand_event(ExternalDemandKind::ExternalCursor, 21, None),
        );
        let third = reduce(
            &second.next,
            external_demand_event(ExternalDemandKind::ExternalCursor, 22, None),
        );

        let third_token = third
            .next
            .timers()
            .active_token(TimerId::Ingress)
            .expect("existing ingress timer token");
        pretty_assert_eq!(third_token, first_token);
        pretty_assert_eq!(
            third.next.demand_queue().latest_cursor(),
            Some(&crate::core::state::QueuedDemand::ready(
                ExternalDemand::new(
                    IngressSeq::new(3),
                    ExternalDemandKind::ExternalCursor,
                    Millis::new(22),
                    None,
                    BufferPerfClass::Full,
                )
            ))
        );
        pretty_assert_eq!(
            second.effects,
            vec![
                Effect::RecordEventLoopMetric(EventLoopMetricEffect::IngressCoalesced),
                Effect::RecordEventLoopMetric(EventLoopMetricEffect::DelayedIngressPendingUpdated,),
            ]
        );
        pretty_assert_eq!(
            third.effects,
            vec![
                Effect::RecordEventLoopMetric(EventLoopMetricEffect::IngressCoalesced),
                Effect::RecordEventLoopMetric(EventLoopMetricEffect::DelayedIngressPendingUpdated,),
            ]
        );
        assert!(
            second
                .effects
                .iter()
                .chain(third.effects.iter())
                .all(|effect| !matches!(
                    effect,
                    Effect::ScheduleTimer(_)
                        | Effect::RecordEventLoopMetric(EventLoopMetricEffect::StaleToken)
                )),
            "burst updates should reuse the original timer generation instead of churning timer tokens"
        );
        pretty_assert_eq!(
            third.next.ingress_policy().pending_delay_until(),
            Some(Millis::new(62))
        );
    }

    #[test]
    fn early_delayed_cursor_timer_fire_rearms_once_for_the_remaining_deadline() {
        let ready = delayed_ready_state();
        let first = reduce(
            &ready,
            external_demand_event(ExternalDemandKind::ExternalCursor, 20, None),
        );
        let delayed = reduce(
            &first.next,
            external_demand_event(ExternalDemandKind::ExternalCursor, 21, None),
        );
        let first_token = delayed
            .next
            .timers()
            .active_token(TimerId::Ingress)
            .expect("ingress timer token");

        let early_fire = reduce(
            &delayed.next,
            Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
                token: first_token,
                observed_at: Millis::new(60),
            }),
        );

        let rearmed_token = early_fire
            .next
            .timers()
            .active_token(TimerId::Ingress)
            .expect("ingress timer should be rearmed for the remaining delay");
        assert_ne!(rearmed_token, first_token);
        pretty_assert_eq!(
            early_fire.effects,
            vec![Effect::ScheduleTimer(ScheduleTimerEffect {
                token: rearmed_token,
                delay: DelayBudgetMs::try_new(1).expect("remaining ingress delay budget"),
                requested_at: Millis::new(60),
            })]
        );
        pretty_assert_eq!(
            early_fire.next.ingress_policy().pending_delay_until(),
            Some(Millis::new(61))
        );
    }

    #[test]
    fn starting_observation_clears_the_pending_delayed_cursor_deadline() {
        let ready = delayed_ready_state();
        let delayed = reduce(
            &ready,
            external_demand_event(ExternalDemandKind::ExternalCursor, 20, None),
        )
        .next;
        let ingress_token = delayed
            .timers()
            .active_token(TimerId::Ingress)
            .expect("ingress timer token");

        let fired = reduce(
            &delayed,
            Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
                token: ingress_token,
                observed_at: Millis::new(60),
            }),
        );

        pretty_assert_eq!(fired.next.ingress_policy().pending_delay_until(), None);
    }
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
            buffer_perf_class: BufferPerfClass::Full,
            ingress_cursor_presentation: None,
        }),
    );

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        transition.effects,
        with_cleanup_invalidation(
            &transition.next,
            24,
            vec![Effect::RequestObservationBase(
                RequestObservationBaseEffect {
                    request: ObservationRequest::new(
                        ExternalDemand::new(
                            IngressSeq::new(1),
                            ExternalDemandKind::ExternalCursor,
                            Millis::new(24),
                            None,
                            BufferPerfClass::Full,
                        ),
                        ProbeRequestSet::new(true, false),
                    ),
                    context: observation_runtime_context(
                        &ready,
                        ExternalDemandKind::ExternalCursor,
                    ),
                }
            )]
        )
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
            buffer_perf_class: BufferPerfClass::Full,
            ingress_cursor_presentation: None,
        }),
    );

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        transition.effects,
        with_cleanup_invalidation(
            &transition.next,
            24,
            vec![Effect::RequestObservationBase(
                RequestObservationBaseEffect {
                    request: ObservationRequest::new(
                        ExternalDemand::new(
                            IngressSeq::new(1),
                            ExternalDemandKind::ExternalCursor,
                            Millis::new(24),
                            None,
                            BufferPerfClass::Full,
                        ),
                        ProbeRequestSet::new(false, true),
                    ),
                    context: observation_runtime_context(
                        &ready,
                        ExternalDemandKind::ExternalCursor,
                    ),
                }
            )]
        )
    );
}

#[test]
fn observation_request_skips_background_probe_for_fast_motion_perf_class() {
    let ready = ready_state_with_runtime_config(|runtime| {
        runtime.config.particles_enabled = true;
        runtime.config.particles_over_text = false;
    });

    let transition = reduce(
        &ready,
        external_demand_event_with_perf_class(
            ExternalDemandKind::ExternalCursor,
            24,
            None,
            BufferPerfClass::FastMotion,
        ),
    );

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        transition.effects,
        with_cleanup_invalidation(
            &transition.next,
            24,
            vec![Effect::RequestObservationBase(
                RequestObservationBaseEffect {
                    request: ObservationRequest::new(
                        ExternalDemand::new(
                            IngressSeq::new(1),
                            ExternalDemandKind::ExternalCursor,
                            Millis::new(24),
                            None,
                            BufferPerfClass::FastMotion,
                        ),
                        ProbeRequestSet::new(false, false),
                    ),
                    context: observation_runtime_context_with_perf_class(
                        &ready,
                        ExternalDemandKind::ExternalCursor,
                        BufferPerfClass::FastMotion,
                    ),
                }
            )]
        )
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
        &CursorLocation::new(44, 55, 9, 10),
    );
    let ready = ready_state().with_runtime(runtime);

    let transition = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(24),
            requested_target: None,
            buffer_perf_class: BufferPerfClass::Full,
            ingress_cursor_presentation: None,
        }),
    );

    pretty_assert_eq!(
        transition.effects,
        with_cleanup_invalidation(
            &transition.next,
            24,
            vec![Effect::RequestObservationBase(
                RequestObservationBaseEffect {
                    request: observation_request(1, ExternalDemandKind::ExternalCursor, 24),
                    context: observation_runtime_context(
                        &ready,
                        ExternalDemandKind::ExternalCursor,
                    ),
                }
            )]
        )
    );
}

#[test]
fn observation_request_reuses_retained_cursor_color_without_downgrading_position_policy() {
    let ready = cursor_color_probe_ready_state().into_ready_with_observation(
        observation_snapshot_with_cursor_color(cursor(3, 4), 0x00AB_CDEF),
    );

    let transition = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(24),
            requested_target: None,
            buffer_perf_class: BufferPerfClass::Full,
            ingress_cursor_presentation: None,
        }),
    );

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        transition.effects,
        with_cleanup_invalidation(
            &transition.next,
            24,
            vec![Effect::RequestObservationBase(
                RequestObservationBaseEffect {
                    request: ObservationRequest::new(
                        ExternalDemand::new(
                            IngressSeq::new(1),
                            ExternalDemandKind::ExternalCursor,
                            Millis::new(24),
                            None,
                            BufferPerfClass::Full,
                        ),
                        ProbeRequestSet::new(true, false),
                    ),
                    context: observation_runtime_context(
                        &ready,
                        ExternalDemandKind::ExternalCursor,
                    ),
                }
            )]
        )
    );
}

#[test]
fn observation_request_uses_fast_motion_probe_quality_for_fast_motion_perf_class() {
    let ready = ready_state();

    let transition = reduce(
        &ready,
        external_demand_event_with_perf_class(
            ExternalDemandKind::ExternalCursor,
            24,
            None,
            BufferPerfClass::FastMotion,
        ),
    );

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        transition.effects,
        with_cleanup_invalidation(
            &transition.next,
            24,
            vec![Effect::RequestObservationBase(
                RequestObservationBaseEffect {
                    request: observation_request_with_perf_class(
                        1,
                        ExternalDemandKind::ExternalCursor,
                        24,
                        BufferPerfClass::FastMotion,
                    ),
                    context: observation_runtime_context_with_perf_class(
                        &ready,
                        ExternalDemandKind::ExternalCursor,
                        BufferPerfClass::FastMotion,
                    ),
                }
            )]
        )
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
            buffer_perf_class: BufferPerfClass::Full,
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

    pretty_assert_eq!(based.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        based.effects,
        vec![Effect::RequestProbe(RequestProbeEffect {
            observation_basis: observation_basis(&request, Some(cursor(7, 8)), 26),
            probe_request_id: ProbeKind::CursorColor.request_id(request.observation_id()),
            kind: ProbeKind::CursorColor,
            cursor_position_policy: cursor_position_policy(&observing),
            buffer_perf_class: request.demand().buffer_perf_class(),
            probe_policy: expected_probe_policy(
                request.demand().kind(),
                request.demand().buffer_perf_class(),
                retained_cursor_color_sample(&observing),
            ),
            background_chunk: None,
            cursor_color_fallback_sample: retained_cursor_color_sample(&observing),
        })]
    );
    match based
        .next
        .observation()
        .expect("active observation snapshot")
        .probes()
        .cursor_color()
    {
        ProbeSlot::Requested(ProbeState::Pending { .. }) => {}
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
            buffer_perf_class: BufferPerfClass::Full,
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

    pretty_assert_eq!(based.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        based.effects,
        vec![Effect::RequestProbe(RequestProbeEffect {
            observation_basis: basis,
            probe_request_id: ProbeKind::CursorColor.request_id(request.observation_id()),
            kind: ProbeKind::CursorColor,
            cursor_position_policy: cursor_position_policy(&observing),
            buffer_perf_class: request.demand().buffer_perf_class(),
            probe_policy: expected_probe_policy(
                request.demand().kind(),
                request.demand().buffer_perf_class(),
                retained_cursor_color_sample(&observing),
            ),
            background_chunk: None,
            cursor_color_fallback_sample: retained_cursor_color_sample(&observing),
        })]
    );
    let observation = based
        .next
        .observation()
        .expect("active observation snapshot");
    assert!(matches!(
        observation.probes().cursor_color(),
        ProbeSlot::Requested(ProbeState::Pending { .. })
    ));
    assert!(
        !observation.probes().background().is_pending(),
        "background probing should not preempt the cursor-color request when no sparse plan is active",
    );
}

#[test]
fn observation_base_collection_keeps_exact_position_policy_when_reusing_retained_cursor_color() {
    let ready = cursor_color_probe_ready_state().into_ready_with_observation(
        observation_snapshot_with_cursor_color(cursor(3, 4), 0x00AB_CDEF),
    );
    let observing = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(25),
            requested_target: None,
            buffer_perf_class: BufferPerfClass::Full,
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

    pretty_assert_eq!(based.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        based.effects,
        vec![Effect::RequestProbe(RequestProbeEffect {
            observation_basis: observation_basis(&request, Some(cursor(7, 8)), 26),
            probe_request_id: ProbeKind::CursorColor.request_id(request.observation_id()),
            kind: ProbeKind::CursorColor,
            cursor_position_policy: cursor_position_policy(&observing),
            buffer_perf_class: request.demand().buffer_perf_class(),
            probe_policy: expected_probe_policy(
                request.demand().kind(),
                request.demand().buffer_perf_class(),
                Some(CursorColorSample::new(0x00AB_CDEF)),
            ),
            background_chunk: None,
            cursor_color_fallback_sample: Some(CursorColorSample::new(0x00AB_CDEF)),
        })]
    );
}

#[test]
fn observation_base_collection_uses_fast_motion_probe_quality_for_fast_motion_perf_class() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.cursor_color = Some("none".to_string());
    let ready = ready_state().with_runtime(runtime);
    let observing = reduce(
        &ready,
        external_demand_event_with_perf_class(
            ExternalDemandKind::ExternalCursor,
            25,
            None,
            BufferPerfClass::FastMotion,
        ),
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

    pretty_assert_eq!(based.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        based.effects,
        vec![Effect::RequestProbe(RequestProbeEffect {
            observation_basis: observation_basis(&request, Some(cursor(7, 8)), 26),
            probe_request_id: ProbeKind::CursorColor.request_id(request.observation_id()),
            kind: ProbeKind::CursorColor,
            cursor_position_policy: cursor_position_policy(&observing),
            buffer_perf_class: BufferPerfClass::FastMotion,
            probe_policy: expected_probe_policy(
                request.demand().kind(),
                request.demand().buffer_perf_class(),
                None,
            ),
            background_chunk: None,
            cursor_color_fallback_sample: None,
        })]
    );
}

#[test]
fn observation_base_collection_skips_cursor_color_probe_when_current_mode_has_explicit_color() {
    let ready = ready_state_with_runtime_config(|runtime| {
        runtime.config.cursor_color = Some("#112233".to_string());
        runtime.config.cursor_color_insert_mode = Some("none".to_string());
    });
    let observing = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(25),
            requested_target: None,
            buffer_perf_class: BufferPerfClass::Full,
            ingress_cursor_presentation: None,
        }),
    )
    .next;
    let request = observing
        .active_observation_request()
        .cloned()
        .expect("active observation");
    let basis = observation_basis_in_mode(&request, Some(cursor(7, 8)), 26, "n");

    let based = reduce(
        &observing,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request,
            basis,
            motion: observation_motion(),
        }),
    );

    pretty_assert_eq!(based.next.lifecycle(), Lifecycle::Planning);
    assert!(matches!(
        based.effects.as_slice(),
        [Effect::RequestRenderPlan(_)]
    ));
    match based
        .next
        .observation()
        .expect("completed observation snapshot")
        .probes()
        .cursor_color()
    {
        ProbeSlot::Requested(ProbeState::Ready { reuse, value, .. }) => {
            pretty_assert_eq!(*reuse, ProbeReuse::Exact);
            pretty_assert_eq!(*value, None);
        }
        other => panic!("expected completed cursor color probe, got {other:?}"),
    }
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
            buffer_perf_class: BufferPerfClass::Full,
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
        cursor_color_probe_report(&request, ProbeReuse::Compatible, Some(0x00AB_CDEF)),
    );

    let observation = completed
        .next
        .observation()
        .expect("stored observation snapshot");
    pretty_assert_eq!(observation.cursor_color(), Some(0x00AB_CDEF));
    match observation.probes().cursor_color() {
        ProbeSlot::Requested(ProbeState::Ready { reuse, .. }) => {
            pretty_assert_eq!(*reuse, ProbeReuse::Compatible)
        }
        other => panic!("expected ready cursor color probe, got {other:?}"),
    }
}

mod probe_completion_sequence {
    use super::*;

    fn dual_probe_scenario() -> ObservationScenario {
        ObservationScenario::with_background_plan(
            dual_probe_ready_state(),
            vec![ScreenCell::new(7, 8).expect("background probe cell")],
        )
    }

    fn background_probe_scenario() -> ObservationScenario {
        ObservationScenario::with_background_probe_cell_count(background_probe_ready_state(), 2050)
    }

    fn single_background_probe_scenario() -> ObservationScenario {
        ObservationScenario::with_background_plan(
            background_probe_ready_state(),
            vec![ScreenCell::new(7, 8).expect("background probe cell")],
        )
    }

    fn after_cursor_color_probe_ready() -> (ObservationScenario, Transition) {
        let scenario = dual_probe_scenario();
        let after_cursor = reduce(
            &scenario.based.next,
            cursor_color_probe_report(&scenario.request, ProbeReuse::Compatible, Some(0x00AB_CDEF)),
        );
        (scenario, after_cursor)
    }

    fn after_first_background_chunk() -> (ObservationScenario, BackgroundProbeChunk, Transition) {
        let scenario = background_probe_scenario();
        let first_chunk = scenario
            .based
            .next
            .observation()
            .and_then(|observation| observation.background_progress())
            .and_then(crate::core::state::BackgroundProbeProgress::next_chunk)
            .expect("first chunk");
        let after_first_chunk = reduce(
            &scenario.based.next,
            background_chunk_probe_report(
                &scenario.request,
                &first_chunk,
                scenario.basis.viewport(),
                &[(7, 8)],
            ),
        );
        (scenario, first_chunk, after_first_chunk)
    }

    #[test]
    fn cursor_color_completion_keeps_observation_active_until_background_probe_finishes() {
        let (_scenario, after_cursor) = after_cursor_color_probe_ready();

        pretty_assert_eq!(after_cursor.next.lifecycle(), Lifecycle::Observing);
        assert!(after_cursor.next.pending_proposal().is_none());
    }

    #[test]
    fn cursor_color_completion_requests_the_first_background_probe_chunk() {
        let (scenario, after_cursor) = after_cursor_color_probe_ready();
        let first_background_chunk = after_cursor
            .next
            .observation()
            .and_then(|observation| observation.background_progress())
            .and_then(crate::core::state::BackgroundProbeProgress::next_chunk)
            .expect("first background probe chunk");

        pretty_assert_eq!(
            after_cursor.effects,
            vec![Effect::RequestProbe(RequestProbeEffect {
                observation_basis: scenario.basis.clone(),
                probe_request_id: ProbeKind::Background
                    .request_id(scenario.request.observation_id()),
                kind: ProbeKind::Background,
                cursor_position_policy: cursor_position_policy(&scenario.based.next),
                buffer_perf_class: scenario.request.demand().buffer_perf_class(),
                probe_policy: expected_probe_policy(
                    scenario.request.demand().kind(),
                    scenario.request.demand().buffer_perf_class(),
                    Some(CursorColorSample::new(0x00AB_CDEF)),
                ),
                background_chunk: Some(first_background_chunk),
                cursor_color_fallback_sample: None,
            })]
        );
    }

    #[test]
    fn cursor_color_completion_retains_cursor_color_while_background_probe_is_pending() {
        let (_scenario, after_cursor) = after_cursor_color_probe_ready();

        let observation = after_cursor
            .next
            .observation()
            .expect("observation should stay active while background probe is pending");
        pretty_assert_eq!(observation.cursor_color(), Some(0x00AB_CDEF));
        assert!(matches!(
            observation.probes().background(),
            ProbeSlot::Requested(ProbeState::Pending { .. })
        ));
    }

    #[test]
    fn final_background_probe_completion_enters_planning_and_requests_render_plan() {
        let (scenario, after_cursor) = after_cursor_color_probe_ready();
        let first_chunk = after_cursor
            .next
            .observation()
            .and_then(|observation| observation.background_progress())
            .and_then(crate::core::state::BackgroundProbeProgress::next_chunk)
            .expect("first background chunk");

        let after_background = reduce(
            &after_cursor.next,
            background_chunk_probe_report(
                &scenario.request,
                &first_chunk,
                scenario.basis.viewport(),
                &[(7, 8)],
            ),
        );

        pretty_assert_eq!(after_background.next.lifecycle(), Lifecycle::Planning);
        assert!(after_background.next.pending_proposal().is_none());
        assert!(after_background.next.pending_plan_proposal_id().is_some());
        assert!(matches!(
            after_background.effects.as_slice(),
            [Effect::RequestRenderPlan(_)]
        ));
    }

    #[test]
    fn observation_base_collection_requests_the_first_background_probe_chunk() {
        let scenario = background_probe_scenario();
        let first_chunk = scenario
            .based
            .next
            .observation()
            .and_then(|observation| observation.background_progress())
            .and_then(crate::core::state::BackgroundProbeProgress::next_chunk);

        pretty_assert_eq!(scenario.based.next.lifecycle(), Lifecycle::Observing);
        pretty_assert_eq!(
            scenario.based.effects,
            vec![Effect::RequestProbe(RequestProbeEffect {
                observation_basis: scenario.basis.clone(),
                probe_request_id: ProbeKind::Background
                    .request_id(scenario.request.observation_id()),
                kind: ProbeKind::Background,
                cursor_position_policy: cursor_position_policy(&scenario.observing),
                buffer_perf_class: scenario.request.demand().buffer_perf_class(),
                probe_policy: expected_probe_policy(
                    scenario.request.demand().kind(),
                    scenario.request.demand().buffer_perf_class(),
                    retained_cursor_color_sample(&scenario.observing),
                ),
                background_chunk: first_chunk,
                cursor_color_fallback_sample: None,
            })]
        );
    }

    #[test]
    fn background_probe_report_stores_allowed_cells_in_snapshot() {
        let scenario = single_background_probe_scenario();
        let first_chunk = scenario
            .based
            .next
            .observation()
            .and_then(|observation| observation.background_progress())
            .and_then(crate::core::state::BackgroundProbeProgress::next_chunk)
            .expect("single background chunk");
        let resolved = reduce(
            &scenario.based.next,
            background_chunk_probe_report(
                &scenario.request,
                &first_chunk,
                scenario.basis.viewport(),
                &[(7, 8)],
            ),
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
    }

    #[test]
    fn background_probe_report_stores_probe_reuse_state_in_snapshot() {
        let scenario = single_background_probe_scenario();
        let resolved = reduce(
            &scenario.based.next,
            background_probe_report(
                &scenario.request,
                scenario.basis.viewport(),
                &[(7, 8)],
                ProbeReuse::Exact,
            ),
        );

        let observation = resolved
            .next
            .observation()
            .expect("stored observation snapshot");
        match observation.probes().background() {
            ProbeSlot::Requested(ProbeState::Ready { reuse, .. }) => {
                pretty_assert_eq!(*reuse, ProbeReuse::Exact)
            }
            other => panic!("expected ready background probe, got {other:?}"),
        }
    }

    #[test]
    fn background_chunk_completion_advances_progress_without_materializing_probe_batch() {
        let (_scenario, first_chunk, after_first_chunk) = after_first_background_chunk();

        let progressed_observation = after_first_chunk
            .next
            .observation()
            .expect("observation should remain active");
        let progressed = progressed_observation
            .background_progress()
            .expect("background progress after first chunk");
        pretty_assert_eq!(
            progressed.next_cell_index(),
            first_chunk.start_index().saturating_add(first_chunk.len())
        );
        assert!(progressed_observation.background_probe().is_none());
    }

    #[test]
    fn background_chunk_completion_requests_the_next_chunk_before_planning() {
        let (scenario, _first_chunk, after_first_chunk) = after_first_background_chunk();
        let progressed = after_first_chunk
            .next
            .observation()
            .and_then(|observation| observation.background_progress())
            .expect("background progress after first chunk");
        let next_chunk = progressed.next_chunk().expect("second chunk");

        pretty_assert_eq!(after_first_chunk.next.lifecycle(), Lifecycle::Observing);
        assert!(after_first_chunk.next.pending_proposal().is_none());
        pretty_assert_eq!(
            after_first_chunk.effects,
            vec![Effect::RequestProbe(RequestProbeEffect {
                observation_basis: scenario.basis,
                probe_request_id: ProbeKind::Background
                    .request_id(scenario.request.observation_id()),
                kind: ProbeKind::Background,
                cursor_position_policy: cursor_position_policy(&scenario.based.next),
                buffer_perf_class: scenario.request.demand().buffer_perf_class(),
                probe_policy: expected_probe_policy(
                    scenario.request.demand().kind(),
                    scenario.request.demand().buffer_perf_class(),
                    retained_cursor_color_sample(&scenario.based.next),
                ),
                background_chunk: Some(next_chunk),
                cursor_color_fallback_sample: None,
            })]
        );
    }
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
            buffer_perf_class: BufferPerfClass::Full,
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

    pretty_assert_eq!(retried.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        retried.effects,
        vec![
            Effect::RecordEventLoopMetric(EventLoopMetricEffect::ProbeRefreshRetried(
                ProbeKind::CursorColor,
            )),
            Effect::RequestObservationBase(RequestObservationBaseEffect {
                request,
                context: observation_runtime_context(
                    &based.next,
                    ExternalDemandKind::ExternalCursor
                ),
            })
        ]
    );
    assert!(retried.next.observation().is_none());
    pretty_assert_eq!(
        retried
            .next
            .probe_refresh_state()
            .expect("probe refresh state while observing")
            .retry_count(ProbeKind::CursorColor),
        1
    );
}

mod probe_refresh_retry_budget {
    use super::*;

    fn exhausted_refresh_transition() -> (ObservationRequest, Transition) {
        let scenario = ObservationScenario::new(cursor_color_probe_ready_state());
        let queued_newer = reduce(
            &scenario.based.next,
            external_demand_event(ExternalDemandKind::ExternalCursor, 27, Some(cursor(9, 10))),
        )
        .next;

        let retry_once = reduce(
            &queued_newer,
            cursor_color_probe_report(&scenario.request, ProbeReuse::RefreshRequired, None),
        );
        let retry_once_based = collect_observation_base(
            &retry_once.next,
            &scenario.request,
            observation_basis(&scenario.request, Some(cursor(7, 9)), 28),
            observation_motion(),
        );
        let retry_twice = reduce(
            &retry_once_based.next,
            cursor_color_probe_report(&scenario.request, ProbeReuse::RefreshRequired, None),
        );
        let retry_twice_based = collect_observation_base(
            &retry_twice.next,
            &scenario.request,
            observation_basis(&scenario.request, Some(cursor(7, 10)), 29),
            observation_motion(),
        );
        let exhausted = reduce(
            &retry_twice_based.next,
            cursor_color_probe_report(&scenario.request, ProbeReuse::RefreshRequired, None),
        );
        (scenario.request, exhausted)
    }

    #[test]
    fn refresh_budget_exhaustion_promotes_the_newer_ingress_request() {
        let (request, exhausted) = exhausted_refresh_transition();

        let replacement_request = exhausted
            .next
            .active_observation_request()
            .cloned()
            .expect("newer ingress should take over after retry budget exhaustion");
        assert_ne!(replacement_request, request);
        pretty_assert_eq!(
            replacement_request.demand().requested_target(),
            Some(cursor(9, 10))
        );
        pretty_assert_eq!(exhausted.next.lifecycle(), Lifecycle::Observing);
    }

    #[test]
    fn refresh_budget_exhaustion_requests_a_new_base_for_the_replacement_ingress() {
        let (_request, exhausted) = exhausted_refresh_transition();
        let replacement_request = exhausted
            .next
            .active_observation_request()
            .cloned()
            .expect("replacement request after retry exhaustion");

        pretty_assert_eq!(
            exhausted.effects,
            vec![
                Effect::RecordEventLoopMetric(EventLoopMetricEffect::ProbeRefreshBudgetExhausted(
                    ProbeKind::CursorColor
                ),),
                Effect::RequestObservationBase(RequestObservationBaseEffect {
                    request: replacement_request,
                    context: observation_runtime_context(
                        &exhausted.next,
                        ExternalDemandKind::ExternalCursor,
                    ),
                }),
            ]
        );
    }

    #[test]
    fn refresh_budget_exhaustion_clears_the_stale_observation_snapshot() {
        let (_request, exhausted) = exhausted_refresh_transition();

        assert!(exhausted.next.observation().is_none());
    }
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
            buffer_perf_class: BufferPerfClass::Full,
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
    pretty_assert_eq!(observation.cursor_color(), None);
    match observation.probes().cursor_color() {
        ProbeSlot::Requested(ProbeState::Failed { failure, .. }) => {
            pretty_assert_eq!(*failure, ProbeFailure::ShellReadFailed)
        }
        other => panic!("expected failed cursor color probe, got {other:?}"),
    }
}

mod observation_completion_planning {
    use super::*;

    fn completed_mode_change_observation_with_cursor_queued() -> Transition {
        let ready = ready_state();
        let observing =
            observing_state_from_demand(&ready, ExternalDemandKind::ModeChanged, 30, None);
        let observing_with_cursor_queued = reduce(
            &observing,
            external_demand_event(ExternalDemandKind::ExternalCursor, 31, None),
        )
        .next;
        let request = active_request(&observing_with_cursor_queued);

        collect_observation_base(
            &observing_with_cursor_queued,
            &request,
            observation_basis(&request, Some(cursor(7, 8)), 32),
            observation_motion(),
        )
    }

    #[test]
    fn observation_completion_enters_planning_and_requests_render_plan() {
        let completed = completed_mode_change_observation_with_cursor_queued();

        pretty_assert_eq!(completed.next.lifecycle(), Lifecycle::Planning);
        pretty_assert_eq!(completed.effects.len(), 1);
        match &completed.effects[0] {
            Effect::RequestRenderPlan(payload) => {
                pretty_assert_eq!(payload.requested_at, Millis::new(32));
                pretty_assert_eq!(
                    Some(payload.proposal_id),
                    completed.next.pending_plan_proposal_id()
                );
            }
            other => panic!("expected first render plan request, got {other:?}"),
        }
    }

    #[test]
    fn observation_completion_keeps_the_newer_cursor_demand_queued_until_planning_finishes() {
        let completed = completed_mode_change_observation_with_cursor_queued();

        assert!(completed.next.demand_queue().latest_cursor().is_some());
    }
}

#[test]
fn cursor_autocmd_demands_refresh_ingress_policy_state() {
    let transition = reduce(
        &ready_state(),
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ModeChanged,
            observed_at: Millis::new(77),
            requested_target: None,
            buffer_perf_class: BufferPerfClass::Full,
            ingress_cursor_presentation: None,
        }),
    );

    pretty_assert_eq!(
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
            buffer_perf_class: BufferPerfClass::Full,
            ingress_cursor_presentation: Some(IngressCursorPresentationRequest::new(
                true,
                true,
                Some(cell),
            )),
        }),
    );

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        transition.effects,
        with_cleanup_invalidation(
            &transition.next,
            78,
            vec![
                Effect::ApplyIngressCursorPresentation(
                    IngressCursorPresentationEffect::HideCursorAndPrepaint {
                        cell,
                        zindex: state.runtime().config.windows_zindex,
                    },
                ),
                Effect::RequestObservationBase(RequestObservationBaseEffect {
                    request: observation_request(1, ExternalDemandKind::ExternalCursor, 78),
                    context: observation_runtime_context(
                        &state,
                        ExternalDemandKind::ExternalCursor,
                    ),
                }),
            ],
        )
    );
}

#[test]
fn animation_timer_from_ready_enters_planning_and_requests_render_plan() {
    let base = ready_state_with_observation(cursor(9, 9));
    let (state, token) = timer_armed_state(base);

    let transition = reduce(&state, animation_tick_event(token, 50));

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Planning);
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
        observation_motion(),
    );
    let mut runtime = ready_state().runtime().clone();
    runtime.initialize_cursor(
        Point { row: 9.0, col: 9.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(11, 22, 3, 4),
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
    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Planning);
    let planned_render = crate::core::reducer::build_planned_render(
        &payload.planning_state,
        payload.proposal_id,
        &payload.render_decision,
        payload.animation_schedule,
    )
    .expect("animation planning should preserve proposal shape invariants");
    let proposal = planned_render.proposal();
    let RealizationPlan::Draw(_) = proposal.realization() else {
        panic!(
            "expected draw proposal after timer-driven drain progress, got {:?}",
            proposal.realization()
        );
    };
    let next_animation_at = proposal
        .animation_schedule()
        .deadline()
        .expect("draw proposal should schedule another animation tick");
    assert!(
        next_animation_at.value() > 116,
        "next animation deadline should advance from timer time, got {}",
        next_animation_at.value()
    );
}

#[test]
fn animation_timer_keeps_rendering_compatible_cursor_color_observation_while_runtime_is_animating()
{
    let base = compatible_cursor_color_ready_state(|runtime| {
        runtime.start_animation();
        runtime.set_last_tick_ms(Some(100.0));
    });
    let (state, token) = timer_armed_state(base);

    let transition = reduce(&state, animation_tick_event(token, 116));

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Planning);
    assert!(matches!(
        transition.effects.as_slice(),
        [Effect::RequestRenderPlan(payload)] if payload.requested_at == Millis::new(116)
    ));
}

#[test]
fn animation_timer_requests_boundary_refresh_for_compatible_cursor_color_when_motion_stops() {
    let base = compatible_cursor_color_ready_state(|_runtime| {});
    let (state, token) = timer_armed_state(base);

    let transition = reduce(&state, animation_tick_event(token, 116));

    let [Effect::RequestObservationBase(payload)] = transition.effects.as_slice() else {
        panic!("expected boundary refresh observation after compatible cursor color reuse");
    };
    let request = active_request(&transition.next);

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(payload.request, request);
    pretty_assert_eq!(payload.context.buffer_perf_class(), BufferPerfClass::Full);
    pretty_assert_eq!(payload.context.probe_quality(), ProbeQuality::Exact);
    pretty_assert_eq!(request.demand().kind(), ExternalDemandKind::BoundaryRefresh);
    pretty_assert_eq!(request.demand().buffer_perf_class(), BufferPerfClass::Full);
    pretty_assert_eq!(request.demand().requested_target(), Some(cursor(9, 9)));
}

#[test]
fn animation_timer_preserves_perf_class_across_boundary_refresh() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.cursor_color = Some("none".to_string());
    runtime.initialize_cursor(
        Point { row: 9.0, col: 9.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(11, 22, 3, 9),
    );
    let request = ObservationRequest::new(
        ExternalDemand::new(
            IngressSeq::new(9),
            ExternalDemandKind::ExternalCursor,
            Millis::new(90),
            None,
            BufferPerfClass::FastMotion,
        ),
        ProbeRequestSet::new(true, false),
    );
    let observation = ObservationSnapshot::new(
        request.clone(),
        observation_basis(&request, Some(cursor(9, 9)), 91),
        observation_motion(),
    )
    .with_cursor_color_probe(ProbeState::ready(
        ProbeKind::CursorColor.request_id(request.observation_id()),
        request.observation_id(),
        ProbeReuse::Compatible,
        Some(CursorColorSample::new(0x00AB_CDEF)),
    ))
    .expect("cursor color probe should be requested");
    let base = ready_state()
        .with_last_cursor(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .into_ready_with_observation(observation);
    let (state, token) = timer_armed_state(base);

    let transition = reduce(&state, animation_tick_event(token, 116));

    let [Effect::RequestObservationBase(payload)] = transition.effects.as_slice() else {
        panic!("expected boundary refresh observation after compatible cursor color reuse");
    };
    let next_request = active_request(&transition.next);

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(payload.request, next_request);
    pretty_assert_eq!(
        payload.context.buffer_perf_class(),
        BufferPerfClass::FastMotion
    );
    pretty_assert_eq!(payload.context.probe_quality(), ProbeQuality::Exact);
    pretty_assert_eq!(
        next_request.demand().buffer_perf_class(),
        BufferPerfClass::FastMotion
    );
    pretty_assert_eq!(
        next_request.demand().kind(),
        ExternalDemandKind::BoundaryRefresh
    );
}

#[test]
fn animation_timer_requests_boundary_refresh_for_conceal_deferred_cursor_position() {
    let base = conceal_deferred_cursor_ready_state(|_runtime| {});
    let (state, token) = timer_armed_state(base);

    let transition = reduce(&state, animation_tick_event(token, 116));

    let [Effect::RequestObservationBase(payload)] = transition.effects.as_slice() else {
        panic!("expected boundary refresh observation after deferred conceal correction");
    };
    let request = active_request(&transition.next);

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(payload.request, request);
    pretty_assert_eq!(
        payload.context.buffer_perf_class(),
        BufferPerfClass::FastMotion
    );
    pretty_assert_eq!(payload.context.probe_quality(), ProbeQuality::Exact);
    pretty_assert_eq!(request.demand().kind(), ExternalDemandKind::BoundaryRefresh);
    pretty_assert_eq!(
        request.demand().buffer_perf_class(),
        BufferPerfClass::FastMotion
    );
    pretty_assert_eq!(request.demand().requested_target(), Some(cursor(9, 9)));
}

#[test]
fn animation_timer_skips_boundary_refresh_when_current_mode_has_explicit_cursor_color() {
    let base = compatible_cursor_color_ready_state(|runtime| {
        runtime.config.cursor_color = Some("#112233".to_string());
        runtime.config.cursor_color_insert_mode = Some("none".to_string());
    });
    let (state, token) = timer_armed_state(base);

    let transition = reduce(&state, animation_tick_event(token, 116));

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Planning);
    assert!(matches!(
        transition.effects.as_slice(),
        [Effect::RequestRenderPlan(payload)] if payload.requested_at == Millis::new(116)
    ));
}

#[test]
fn idle_apply_completion_requests_boundary_refresh_for_compatible_cursor_color() {
    let (applying, proposal_id) = applying_state_with_realization_plan(
        compatible_cursor_color_ready_state(|_runtime| {}),
        noop_realization_plan(),
        false,
        None,
    );

    let transition = reduce(
        &applying,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(101),
            visual_change: false,
        }),
    );

    let request = active_request(&transition.next);
    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(request.demand().kind(), ExternalDemandKind::BoundaryRefresh);
    assert!(transition.effects.iter().any(|effect| {
        matches!(
            effect,
            Effect::RequestObservationBase(payload)
                if payload.request == request && payload.context.probe_quality() == ProbeQuality::Exact
        )
    }));
}

#[test]
fn idle_apply_completion_requests_boundary_refresh_for_conceal_deferred_cursor_position() {
    let (applying, proposal_id) = applying_state_with_realization_plan(
        conceal_deferred_cursor_ready_state(|_runtime| {}),
        noop_realization_plan(),
        false,
        None,
    );

    let transition = reduce(
        &applying,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(101),
            visual_change: false,
        }),
    );

    let request = active_request(&transition.next);
    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(request.demand().kind(), ExternalDemandKind::BoundaryRefresh);
    pretty_assert_eq!(
        request.demand().buffer_perf_class(),
        BufferPerfClass::FastMotion
    );
    assert!(transition.effects.iter().any(|effect| {
        matches!(
            effect,
            Effect::RequestObservationBase(payload)
                if payload.request == request
                    && payload.context.buffer_perf_class() == BufferPerfClass::FastMotion
                    && payload.context.probe_quality() == ProbeQuality::Exact
        )
    }));
}

#[test]
fn observation_completion_with_text_mutation_requests_clear_all_render_plan() {
    let previous_request = observation_request(9, ExternalDemandKind::ExternalCursor, 90);
    let previous_observation = ObservationSnapshot::new(
        previous_request.clone(),
        observation_basis_with_text_context(
            &previous_request,
            Some(cursor(9, 9)),
            91,
            9,
            10,
            &[(8, "before"), (9, "alpha"), (10, "after")],
            None,
            None,
        ),
        observation_motion(),
    );
    let mut runtime = ready_state().runtime().clone();
    runtime.initialize_cursor(
        Point { row: 9.0, col: 9.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(11, 22, 3, 9),
    );
    let ready = ready_state()
        .with_last_cursor(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .into_ready_with_observation(previous_observation);
    let observing = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 100, None),
    )
    .next;
    let request = active_request(&observing);

    let transition = collect_observation_base(
        &observing,
        &request,
        observation_basis_with_text_context(
            &request,
            Some(cursor(9, 10)),
            101,
            9,
            11,
            &[(8, "before"), (9, "alphab"), (10, "after")],
            Some(9),
            Some(&[(8, "before"), (9, "alphab"), (10, "after")]),
        ),
        observation_motion(),
    );

    let [Effect::RequestRenderPlan(payload)] = transition.effects.as_slice() else {
        panic!("expected render plan request after text mutation observation");
    };
    assert!(matches!(
        payload.render_decision.render_action,
        RenderAction::ClearAll
    ));
}

#[test]
fn observation_completion_with_motion_only_requests_draw_render_plan() {
    let previous_request = observation_request(9, ExternalDemandKind::ExternalCursor, 90);
    let previous_observation = ObservationSnapshot::new(
        previous_request.clone(),
        observation_basis_with_text_context(
            &previous_request,
            Some(cursor(9, 9)),
            91,
            9,
            10,
            &[(8, "before"), (9, "alpha"), (10, "after")],
            None,
            None,
        ),
        observation_motion(),
    );
    let mut runtime = ready_state().runtime().clone();
    runtime.initialize_cursor(
        Point { row: 9.0, col: 9.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(11, 22, 3, 9),
    );
    let ready = ready_state()
        .with_last_cursor(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .into_ready_with_observation(previous_observation);
    let observing = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 100, None),
    )
    .next;
    let request = active_request(&observing);

    let transition = collect_observation_base(
        &observing,
        &request,
        observation_basis_with_text_context(
            &request,
            Some(cursor(10, 9)),
            101,
            10,
            10,
            &[(9, "alpha"), (10, "after"), (11, "tail")],
            Some(9),
            Some(&[(8, "before"), (9, "alpha"), (10, "after")]),
        ),
        observation_motion(),
    );

    let [Effect::RequestRenderPlan(payload)] = transition.effects.as_slice() else {
        panic!("expected render plan request after motion-only observation");
    };
    assert!(matches!(
        payload.render_decision.render_action,
        RenderAction::Draw(_)
    ));
}

#[test]
fn observation_completion_with_scroll_and_text_mutation_still_requests_clear_all_render_plan() {
    let previous_request = observation_request(9, ExternalDemandKind::ExternalCursor, 90);
    let previous_observation = ObservationSnapshot::new(
        previous_request.clone(),
        observation_basis_with_text_context(
            &previous_request,
            Some(cursor(9, 9)),
            91,
            9,
            10,
            &[(8, "before"), (9, "alpha"), (10, "after")],
            None,
            None,
        ),
        observation_motion(),
    );
    let mut runtime = ready_state().runtime().clone();
    runtime.initialize_cursor(
        Point { row: 9.0, col: 9.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(11, 22, 1, 9),
    );
    let ready = ready_state()
        .with_last_cursor(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .into_ready_with_observation(previous_observation);
    let observing = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 100, None),
    )
    .next;
    let request = active_request(&observing);

    let transition = collect_observation_base(
        &observing,
        &request,
        ObservationBasis::new(
            request.observation_id(),
            Millis::new(101),
            "n".to_string(),
            Some(cursor(10, 3)),
            CursorLocation::new(11, 22, 4, 10),
            ViewportSnapshot::new(CursorRow(40), CursorCol(120)),
        )
        .with_cursor_text_context(Some(text_context(
            11,
            10,
            &[(9, "alpha pasted"), (10, "block"), (11, "tail")],
            Some(9),
            Some(&[(8, "before"), (9, "alpha pasted"), (10, "block")]),
        ))),
        observation_motion(),
    );

    let [Effect::RequestRenderPlan(payload)] = transition.effects.as_slice() else {
        panic!("expected render plan request after scroll + text mutation observation");
    };
    assert!(matches!(
        payload.render_decision.render_action,
        RenderAction::ClearAll
    ));
}

mod animation_timer_draw_state {
    use super::*;

    fn staged_draw_state() -> CoreState {
        planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), 61).0
    }

    #[test]
    fn animation_timer_draw_advances_scene_revision_and_marks_the_trail_dirty() {
        let state = staged_draw_state();
        let scene = state.scene();

        pretty_assert_eq!(scene.revision().value(), 1);
        pretty_assert_eq!(
            scene.dirty().entities(),
            &std::collections::BTreeSet::from([SemanticEntityId::CursorTrail])
        );
    }

    #[test]
    fn animation_timer_draw_populates_projection_cache_from_the_retained_observation() {
        let state = staged_draw_state();
        let projection = state
            .scene()
            .projection_entry()
            .expect("projection cache after draw render")
            .snapshot()
            .clone();

        pretty_assert_eq!(projection.witness().observation_id().value(), 9);
        pretty_assert_eq!(
            projection.witness().viewport(),
            ViewportSnapshot::new(CursorRow(40), CursorCol(120))
        );
        pretty_assert_eq!(
            projection
                .logical_raster()
                .clear()
                .map(|clear| clear.max_kept_windows),
            Some(state.runtime().config.max_kept_windows)
        );
    }

    #[test]
    fn animation_timer_draw_stages_a_draw_proposal_against_the_projection_cache_target() {
        let state = staged_draw_state();
        let scene = state.scene();
        let projection = scene
            .projection_entry()
            .expect("projection cache entry after draw render")
            .snapshot()
            .clone();
        let Some(proposal) = state.pending_proposal() else {
            panic!("expected staged render proposal");
        };
        let RealizationPlan::Draw(draw) = proposal.realization() else {
            panic!("expected draw realization plan");
        };

        pretty_assert_eq!(
            scene
                .projection_entry()
                .expect("projection cache entry after draw render")
                .reuse_key()
                .target_cell_presentation(),
            proposal.side_effects().target_cell_presentation
        );
        pretty_assert_eq!(
            draw.palette().color_levels(),
            state.runtime().config.color_levels
        );
        pretty_assert_eq!(
            draw.max_kept_windows(),
            state.runtime().config.max_kept_windows
        );
        pretty_assert_eq!(proposal.patch().basis().target(), Some(&projection));
    }
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

    pretty_assert_eq!(completed.next.lifecycle(), Lifecycle::Ready);
    pretty_assert_eq!(
        completed.next.realization(),
        &RealizationLedger::Consistent { acknowledged }
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

    pretty_assert_eq!(cleaned.next.lifecycle(), Lifecycle::Ready);
    pretty_assert_eq!(
        cleaned.next.realization(),
        &RealizationLedger::Diverged {
            last_consistent: Some(acknowledged.clone()),
            divergence: RealizationDivergence::ShellStateUnknown,
        }
    );
    pretty_assert_eq!(
        cleaned.next.realization().trusted_acknowledged_for_patch(),
        None
    );
    pretty_assert_eq!(
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

    pretty_assert_eq!(patch.kind(), crate::core::state::ScenePatchKind::Replace);
}

#[test]
fn apply_completion_emits_explicit_cleanup_and_redraw_effects() {
    let mut runtime = ready_state_with_observation(cursor(4, 9)).runtime().clone();
    runtime.config.max_kept_windows = 21;
    let state = ready_state_with_observation(cursor(4, 9)).with_runtime(runtime.clone());
    let basis = PatchBasis::new(None, None);
    let patch = ScenePatch::derive(basis);
    let (state, proposal_id) = state.allocate_proposal_id();
    let proposal = InFlightProposal::clear(
        proposal_id,
        patch,
        RealizationClear::new(21),
        RenderCleanupAction::Schedule,
        RenderSideEffects {
            redraw_after_clear_if_cmdline: true,
            ..RenderSideEffects::default()
        },
        crate::core::state::AnimationSchedule::Idle,
    )
    .expect("clear proposal should be constructible");
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

    pretty_assert_eq!(completed.next.lifecycle(), Lifecycle::Ready);
    pretty_assert_eq!(
        completed.next.render_cleanup().thermal(),
        RenderThermalState::Hot
    );
    pretty_assert_eq!(
        completed.next.render_cleanup().next_compaction_due_at(),
        Some(Millis::new(79 + render_cleanup_delay_ms(&runtime.config)))
    );
    pretty_assert_eq!(completed.next.render_cleanup().entered_cooling_at(), None);
    let cleanup_token = completed
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("cleanup timer should be armed");
    pretty_assert_eq!(
        completed.effects,
        vec![
            Effect::ScheduleTimer(ScheduleTimerEffect {
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
fn cleanup_timer_soft_clear_immediately_emits_first_cooling_compaction() {
    let mut runtime = ready_state_with_observation(cursor(4, 9)).runtime().clone();
    runtime.config.max_kept_windows = 21;
    let state = ready_state_with_observation(cursor(4, 9)).with_runtime(runtime.clone());
    let basis = PatchBasis::new(None, None);
    let patch = ScenePatch::derive(basis);
    let (state, proposal_id) = state.allocate_proposal_id();
    let proposal = InFlightProposal::clear(
        proposal_id,
        patch,
        RealizationClear::new(21),
        RenderCleanupAction::Schedule,
        RenderSideEffects::default(),
        crate::core::state::AnimationSchedule::Idle,
    )
    .expect("clear proposal should be constructible");
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
    pretty_assert_eq!(
        completed.next.render_cleanup().thermal(),
        RenderThermalState::Hot
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

    pretty_assert_eq!(
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
    pretty_assert_eq!(
        after_soft.next.render_cleanup().thermal(),
        RenderThermalState::Cooling
    );
    pretty_assert_eq!(
        after_soft.next.render_cleanup().entered_cooling_at(),
        Some(Millis::new(79 + render_cleanup_delay_ms(&runtime.config)))
    );
    pretty_assert_eq!(
        after_soft.next.timers().active_token(TimerId::Cleanup),
        None
    );
    pretty_assert_eq!(
        after_soft.effects,
        vec![Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
            execution: RenderCleanupExecution::CompactToBudget {
                target_budget: 2,
                max_prune_per_tick: 21,
            },
        })]
    );

    let after_compaction = reduce(
        &after_soft.next,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
            action: RenderCleanupAppliedAction::CompactedToBudget {
                converged_to_idle: true,
            },
        }),
    );

    pretty_assert_eq!(
        after_compaction
            .next
            .timers()
            .active_token(TimerId::Cleanup),
        None
    );
    pretty_assert_eq!(
        after_compaction.next.render_cleanup().thermal(),
        RenderThermalState::Cold
    );
    pretty_assert_eq!(
        after_compaction.next.render_cleanup().idle_target_budget(),
        2
    );
    pretty_assert_eq!(
        after_compaction.next.render_cleanup().max_kept_windows(),
        21
    );
    pretty_assert_eq!(
        after_compaction.effects,
        vec![Effect::RecordEventLoopMetric(
            EventLoopMetricEffect::CleanupConvergedToCold {
                started_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
                converged_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
            },
        )]
    );
}

#[test]
fn hard_purge_stays_as_fallback_when_cooling_compaction_does_not_converge() {
    let mut runtime = ready_state_with_observation(cursor(4, 9)).runtime().clone();
    runtime.config.max_kept_windows = 21;
    let state = ready_state_with_observation(cursor(4, 9)).with_runtime(runtime.clone());
    let basis = PatchBasis::new(None, None);
    let patch = ScenePatch::derive(basis);
    let (state, proposal_id) = state.allocate_proposal_id();
    let proposal = InFlightProposal::clear(
        proposal_id,
        patch,
        RealizationClear::new(21),
        RenderCleanupAction::Schedule,
        RenderSideEffects::default(),
        crate::core::state::AnimationSchedule::Idle,
    )
    .expect("clear proposal should be constructible");
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
    let after_soft = reduce(
        &soft_tick.next,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
            action: RenderCleanupAppliedAction::SoftCleared,
        }),
    );
    pretty_assert_eq!(
        after_soft.next.timers().active_token(TimerId::Cleanup),
        None
    );
    let after_compaction = reduce(
        &after_soft.next,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
            action: RenderCleanupAppliedAction::CompactedToBudget {
                converged_to_idle: false,
            },
        }),
    );
    pretty_assert_eq!(
        after_compaction.next.render_cleanup().thermal(),
        RenderThermalState::Cooling
    );

    let hard_token = after_compaction
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("hard purge fallback timer should stay armed while cooling remains pending");

    let hard_tick = reduce(
        &after_compaction.next,
        cleanup_tick_event(
            hard_token,
            79 + render_hard_cleanup_delay_ms(&runtime.config),
        ),
    );

    pretty_assert_eq!(
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

    pretty_assert_eq!(
        after_hard.next.timers().active_token(TimerId::Cleanup),
        None
    );
    pretty_assert_eq!(
        after_hard.next.render_cleanup().thermal(),
        RenderThermalState::Cold
    );
    pretty_assert_eq!(after_hard.next.render_cleanup().idle_target_budget(), 2);
    pretty_assert_eq!(after_hard.next.render_cleanup().max_kept_windows(), 21);
    pretty_assert_eq!(
        after_hard.effects,
        vec![Effect::RecordEventLoopMetric(
            EventLoopMetricEffect::CleanupConvergedToCold {
                started_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
                converged_at: Millis::new(79 + render_hard_cleanup_delay_ms(&runtime.config)),
            },
        )]
    );
}

#[test]
fn fresh_ingress_promotes_cooling_cleanup_state_back_to_hot() {
    let mut runtime = ready_state_with_observation(cursor(4, 9)).runtime().clone();
    runtime.config.max_kept_windows = 21;
    let state = ready_state_with_observation(cursor(4, 9)).with_runtime(runtime.clone());
    let basis = PatchBasis::new(None, None);
    let patch = ScenePatch::derive(basis);
    let (state, proposal_id) = state.allocate_proposal_id();
    let proposal = InFlightProposal::clear(
        proposal_id,
        patch,
        RealizationClear::new(21),
        RenderCleanupAction::Schedule,
        RenderSideEffects::default(),
        crate::core::state::AnimationSchedule::Idle,
    )
    .expect("clear proposal should be constructible");
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
    let cooling = reduce(
        &soft_tick.next,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
            action: RenderCleanupAppliedAction::SoftCleared,
        }),
    )
    .next;

    let reheated = reduce(
        &cooling,
        external_demand_event(ExternalDemandKind::BufferEntered, 150, None),
    );

    pretty_assert_eq!(
        reheated.next.render_cleanup().thermal(),
        RenderThermalState::Hot
    );
    assert!(
        reheated
            .next
            .timers()
            .active_token(TimerId::Cleanup)
            .is_some(),
        "fresh ingress should keep one cleanup timer alive while hot deadlines move forward"
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

    pretty_assert_eq!(patch.kind(), crate::core::state::ScenePatchKind::Replace);
}

mod apply_completion_resume {
    use super::*;

    fn completed_apply_with_pending_mode_change() -> (CoreState, Transition) {
        let (staged, proposal_id) = applying_state_with_realization_plan(
            ready_state_with_observation(cursor(4, 9)),
            noop_realization_plan(),
            true,
            Some(Millis::new(90)),
        );
        let staged = reduce(
            &staged,
            external_demand_event(ExternalDemandKind::ModeChanged, 71, None),
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
        (staged, completed)
    }

    #[test]
    fn apply_completion_clears_the_in_flight_proposal_and_resumes_observing() {
        let (_staged, completed) = completed_apply_with_pending_mode_change();

        pretty_assert_eq!(completed.next.lifecycle(), Lifecycle::Observing);
        pretty_assert_eq!(completed.next.realization(), &RealizationLedger::Cleared);
        assert!(completed.next.pending_proposal().is_none());
    }

    #[test]
    fn apply_completion_requests_the_pending_observation_before_rearming_animation() {
        let (staged, completed) = completed_apply_with_pending_mode_change();

        pretty_assert_eq!(
            completed.effects[0],
            Effect::RequestObservationBase(RequestObservationBaseEffect {
                request: observation_request(1, ExternalDemandKind::ModeChanged, 71),
                context: observation_runtime_context(&staged, ExternalDemandKind::ModeChanged),
            })
        );
    }

    #[test]
    fn apply_completion_rearms_animation_after_requesting_the_pending_observation() {
        let (_staged, completed) = completed_apply_with_pending_mode_change();

        pretty_assert_eq!(completed.effects.len(), 2);
        assert!(matches!(
            completed.effects[1],
            Effect::ScheduleTimer(ScheduleTimerEffect { .. })
        ));
    }
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
            acknowledged: acknowledged.clone(),
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

    pretty_assert_eq!(failed.next.lifecycle(), Lifecycle::Recovering);
    pretty_assert_eq!(
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

    pretty_assert_eq!(failed.next.lifecycle(), Lifecycle::Recovering);
    pretty_assert_eq!(
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

    pretty_assert_eq!(stale.next, staged);
    pretty_assert_eq!(
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

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Recovering);
    pretty_assert_eq!(
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
        [Effect::ScheduleTimer(ScheduleTimerEffect { .. })]
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

    pretty_assert_eq!(degraded.next.lifecycle(), Lifecycle::Recovering);
    pretty_assert_eq!(
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

    pretty_assert_eq!(transition.next, state);
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

    pretty_assert_eq!(failed.next.lifecycle(), Lifecycle::Recovering);
    pretty_assert_eq!(
        failed.next.realization(),
        &RealizationLedger::Diverged {
            last_consistent: Some(acknowledged),
            divergence: RealizationDivergence::ShellStateUnknown,
        }
    );
    assert!(matches!(
        failed.effects.as_slice(),
        [Effect::ScheduleTimer(ScheduleTimerEffect { .. })]
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

    pretty_assert_eq!(stale.next, staged);
    pretty_assert_eq!(
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
            token: stale_token,
            observed_at: Millis::new(120),
        }),
    );

    pretty_assert_eq!(transition.next, state);
    pretty_assert_eq!(
        transition.effects,
        vec![Effect::RecordEventLoopMetric(
            EventLoopMetricEffect::StaleToken
        )]
    );
}

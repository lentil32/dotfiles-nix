use crate::core::effect::ApplyProposalEffect;
use crate::core::effect::ApplyRenderCleanupEffect;
use crate::core::effect::CursorColorFallback;
use crate::core::effect::CursorPositionReadPolicy;
use crate::core::effect::Effect;
use crate::core::effect::EventLoopMetricEffect;
use crate::core::effect::IngressCursorPresentationEffect;
use crate::core::effect::IngressCursorPresentationRequest;
use crate::core::effect::IngressObservationSurface;
use crate::core::effect::ObservationRuntimeContext;
use crate::core::effect::ObservationRuntimeContextArgs;
use crate::core::effect::ProbePolicy;
use crate::core::effect::RenderCleanupExecution;
use crate::core::effect::RenderPlanningContext;
use crate::core::effect::RequestObservationBaseEffect;
use crate::core::effect::RequestProbeEffect;
use crate::core::effect::RequestRenderPlanEffect;
use crate::core::effect::RetainedCursorColorFallback;
use crate::core::effect::ScheduleTimerEffect;
use crate::core::effect::tracked_observation_inputs;
use crate::core::event::RenderCleanupAppliedAction;
use crate::core::runtime_reducer::RenderCleanupAction;
use crate::core::runtime_reducer::RenderDecision;
use crate::core::runtime_reducer::render_cleanup_delay_ms;
use crate::core::runtime_reducer::render_cleanup_idle_target_budget;
use crate::core::runtime_reducer::render_cleanup_max_prune_per_tick;
use crate::core::runtime_reducer::render_hard_cleanup_delay_ms;
use crate::core::state::AnimationSchedule;
use crate::core::state::BufferPerfClass;
use crate::core::state::CoreState;
use crate::core::state::CursorTextContextBoundary;
use crate::core::state::ExternalDemand;
use crate::core::state::ExternalDemandKind;
use crate::core::state::InFlightProposal;
use crate::core::state::ObservationBasis;
use crate::core::state::ObservationSnapshot;
use crate::core::state::PendingObservation;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeRequestSet;
use crate::core::state::ProbeState;
use crate::core::state::ProposalExecution;
use crate::core::state::RenderCleanupState;
use crate::core::types::DelayBudgetMs;
use crate::core::types::Millis;
use crate::core::types::ProposalId;
use crate::core::types::TimerId;

pub(super) const DEFAULT_ANIMATION_DELAY_MS: DelayBudgetMs = DelayBudgetMs::DEFAULT_ANIMATION;
pub(super) const RECOVERY_MAX_ATTEMPTS: u8 = 5;
pub(super) const DEFAULT_RECOVERY_DELAY_MS: u64 = 16;
const RECOVERY_MAX_DELAY_MS: u64 = 256;

pub(super) fn delay_budget_from_ms(delay_ms: u64) -> DelayBudgetMs {
    DelayBudgetMs::try_new(delay_ms).map_or(
        // Surprising: recovery backoff produced a non-positive delay. Fall back to animation default.
        DEFAULT_ANIMATION_DELAY_MS,
        |delay| delay,
    )
}

fn recovery_backoff_delay(attempt: u8) -> DelayBudgetMs {
    if attempt == 0 {
        return delay_budget_from_ms(DEFAULT_RECOVERY_DELAY_MS);
    }

    let shift = u32::from(attempt.saturating_sub(1));
    let scaled = 1_u64
        .checked_shl(shift)
        .map_or(RECOVERY_MAX_DELAY_MS, |scale| {
            DEFAULT_RECOVERY_DELAY_MS.saturating_mul(scale)
        });
    let clamped = scaled.min(RECOVERY_MAX_DELAY_MS);
    delay_budget_from_ms(clamped)
}

fn next_recovery_attempt(state: &CoreState) -> u8 {
    state
        .recovery_policy()
        .retry_attempt()
        .saturating_add(1)
        .min(RECOVERY_MAX_ATTEMPTS)
}

fn with_recovery_attempt(state: CoreState, retry_attempt: u8) -> CoreState {
    let recovery_policy = state.recovery_policy().with_retry_attempt(retry_attempt);
    state.with_recovery_policy(recovery_policy)
}

pub(super) fn reset_recovery_attempt(state: CoreState) -> CoreState {
    with_recovery_attempt(state, 0)
}

pub(super) fn enter_recovering_with_backoff(
    state: CoreState,
    observed_at: Millis,
) -> (CoreState, Effect) {
    let attempt = next_recovery_attempt(&state);
    let recovering = with_recovery_attempt(state.enter_recovering(), attempt);
    let delay = recovery_backoff_delay(attempt);
    schedule_timer_with_delay(recovering, TimerId::Recovery, delay, observed_at)
}

pub(super) fn ingress_marks_cursor_autocmd_freshness(kind: ExternalDemandKind) -> bool {
    matches!(
        kind,
        ExternalDemandKind::ExternalCursor | ExternalDemandKind::ModeChanged
    )
}

fn hot_cleanup_state(state: &CoreState, observed_at: Millis) -> RenderCleanupState {
    RenderCleanupState::scheduled(
        observed_at,
        render_cleanup_delay_ms(&state.runtime().config),
        render_hard_cleanup_delay_ms(&state.runtime().config),
    )
}

fn scheduled_cleanup_state(state: &CoreState, observed_at: Millis) -> RenderCleanupState {
    match state.render_cleanup() {
        RenderCleanupState::Cold => hot_cleanup_state(state, observed_at),
        RenderCleanupState::Hot(_) | RenderCleanupState::Cooling(_) => state.render_cleanup(),
    }
}

pub(super) fn schedule_timer_with_delay(
    state: CoreState,
    timer_id: TimerId,
    delay: DelayBudgetMs,
    requested_at: Millis,
) -> (CoreState, Effect) {
    let (timers, token) = state.timers().arm(timer_id);
    let next_state = state.with_timers(timers);
    let effect = Effect::ScheduleTimer(ScheduleTimerEffect {
        token,
        delay,
        requested_at,
    });
    (next_state, effect)
}

fn cursor_position_read_policy(state: &CoreState) -> CursorPositionReadPolicy {
    CursorPositionReadPolicy::new(state.runtime().config.smear_to_cmd)
}

pub(super) fn observation_cursor_color_fallback(
    observation: Option<&ObservationSnapshot>,
) -> Option<CursorColorFallback> {
    let observation = observation?;
    let sample = observation
        .cursor_color()
        .map(crate::core::state::CursorColorSample::new)?;
    let witness = observation.cursor_color_probe_witness()?;
    Some(CursorColorFallback::new(sample, witness))
}

fn observation_cursor_text_context_boundary(
    observation: Option<&ObservationSnapshot>,
) -> Option<CursorTextContextBoundary> {
    observation.and_then(|snapshot| snapshot.basis().cursor_text_context_state().boundary())
}

pub(super) fn exact_boundary_refresh_required(state: &CoreState) -> bool {
    let Some(observation) = state.phase_observation() else {
        return false;
    };

    if !state.runtime().is_initialized() || state.runtime().is_animating() {
        return false;
    }

    let cursor_color_refresh = state
        .runtime()
        .config
        .requires_cursor_color_sampling_for_mode(observation.basis().mode())
        && observation.requires_exact_cursor_color_refresh();
    let cursor_position_refresh = match observation.basis().cursor().cell() {
        crate::position::ObservedCell::Unavailable => false,
        crate::position::ObservedCell::Exact(_) => false,
        crate::position::ObservedCell::Deferred(_) => true,
    };

    cursor_color_refresh || cursor_position_refresh
}

pub(super) fn start_boundary_refresh_observation(
    state: CoreState,
    observed_at: Millis,
) -> Option<(CoreState, Effect)> {
    if !exact_boundary_refresh_required(&state) {
        return None;
    }

    let buffer_perf_class = state
        .phase_observation()
        .map_or(BufferPerfClass::Full, |observation| {
            observation.demand().buffer_perf_class()
        });
    let (state, seq) = state.allocate_ingress_seq();
    let demand = ExternalDemand::new(
        seq,
        ExternalDemandKind::BoundaryRefresh,
        observed_at,
        buffer_perf_class,
    );
    let pending = PendingObservation::new(demand, probe_requests_for(&state, buffer_perf_class));
    let Some(next_state) = state.enter_observing_request(pending.clone()) else {
        unreachable!("boundary refresh observations should only start from ready states");
    };
    let effect = request_observation_base(&next_state, pending, None);
    Some((next_state, effect))
}

fn observation_runtime_context(
    state: &CoreState,
    pending: &PendingObservation,
    ingress_observation_surface: Option<IngressObservationSurface>,
) -> ObservationRuntimeContext {
    let cursor_color_fallback = observation_cursor_color_fallback(state.phase_observation());
    let cursor_text_context_boundary =
        observation_cursor_text_context_boundary(state.phase_observation());
    let buffer_perf_class = pending.demand().buffer_perf_class();
    let retained_cursor_color_fallback = match cursor_color_fallback {
        Some(_) => RetainedCursorColorFallback::CompatibleSample,
        None => RetainedCursorColorFallback::Unavailable,
    };
    let probe_policy = ProbePolicy::for_demand(
        pending.demand().kind(),
        buffer_perf_class,
        retained_cursor_color_fallback,
    );
    let (tracked_surface, tracked_buffer_position) =
        tracked_observation_inputs(state.runtime().tracked_cursor_ref());
    ObservationRuntimeContext::new(ObservationRuntimeContextArgs {
        cursor_position_policy: cursor_position_read_policy(state),
        scroll_buffer_space: state.runtime().config.scroll_buffer_space,
        tracked_surface,
        tracked_buffer_position,
        cursor_text_context_boundary,
        current_corners: state.runtime().current_corners(),
        ingress_observation_surface,
        buffer_perf_class,
        probe_policy,
    })
}

pub(super) fn request_observation_base(
    state: &CoreState,
    pending: PendingObservation,
    ingress_observation_surface: Option<IngressObservationSurface>,
) -> Effect {
    Effect::RequestObservationBase(RequestObservationBaseEffect {
        context: observation_runtime_context(state, &pending, ingress_observation_surface),
        request: pending,
    })
}

pub(super) fn record_event_loop_metric(metric: EventLoopMetricEffect) -> Effect {
    Effect::RecordEventLoopMetric(metric)
}

pub(super) fn probe_refresh_retry_metric(kind: ProbeKind) -> Effect {
    record_event_loop_metric(EventLoopMetricEffect::ProbeRefreshRetried(kind))
}

pub(super) fn probe_refresh_budget_exhausted_metric(kind: ProbeKind) -> Effect {
    record_event_loop_metric(EventLoopMetricEffect::ProbeRefreshBudgetExhausted(kind))
}

#[expect(
    clippy::too_many_arguments,
    reason = "the reducer keeps probe request policy explicit at the callsite while adaptive perf-class plumbing settles"
)]
fn request_probe(
    state: &CoreState,
    observation_id: crate::core::types::ObservationId,
    observation_basis: ObservationBasis,
    cursor_color_probe_generations: Option<crate::core::state::CursorColorProbeGenerations>,
    kind: ProbeKind,
    background_chunk: Option<crate::core::state::BackgroundProbeChunk>,
    demand_kind: ExternalDemandKind,
    buffer_perf_class: BufferPerfClass,
    cursor_color_fallback: Option<CursorColorFallback>,
) -> Effect {
    let retained_cursor_color_fallback = match cursor_color_fallback {
        Some(_) => RetainedCursorColorFallback::CompatibleSample,
        None => RetainedCursorColorFallback::Unavailable,
    };
    let probe_policy = ProbePolicy::for_demand(
        demand_kind,
        buffer_perf_class,
        retained_cursor_color_fallback,
    );
    Effect::RequestProbe(RequestProbeEffect {
        observation_id,
        observation_basis: Box::new(observation_basis),
        cursor_color_probe_generations,
        kind,
        cursor_position_policy: cursor_position_read_policy(state),
        buffer_perf_class,
        probe_policy,
        background_chunk,
        cursor_color_fallback: if matches!(kind, ProbeKind::CursorColor) {
            cursor_color_fallback
        } else {
            None
        },
    })
}

pub(super) fn probe_requests_for(
    state: &CoreState,
    buffer_perf_class: BufferPerfClass,
) -> ProbeRequestSet {
    let mut requests = ProbeRequestSet::none();

    if state.runtime().config.requires_cursor_color_sampling() {
        requests = requests.with_requested(ProbeKind::CursorColor);
    }

    if state
        .runtime()
        .config
        .requires_background_sampling_for_perf_class(buffer_perf_class)
    {
        requests = requests.with_requested(ProbeKind::Background);
    }

    requests
}

pub(super) fn next_pending_probe_effect(
    state: &CoreState,
    observation: &ObservationSnapshot,
    cursor_color_fallback: Option<CursorColorFallback>,
) -> Option<Effect> {
    let basis = observation.basis();
    let demand_kind = observation.demand().kind();
    let buffer_perf_class = observation.demand().buffer_perf_class();
    if let crate::core::state::ProbeSlot::Requested(ProbeState::Pending) =
        observation.probes().cursor_color()
    {
        return Some(request_probe(
            state,
            observation.observation_id(),
            basis.clone(),
            observation.cursor_color_probe_generations(),
            ProbeKind::CursorColor,
            None,
            demand_kind,
            buffer_perf_class,
            cursor_color_fallback,
        ));
    }

    if let Some(background_chunk) = observation.probes().background().next_chunk() {
        return Some(request_probe(
            state,
            observation.observation_id(),
            basis.clone(),
            None,
            ProbeKind::Background,
            Some(background_chunk),
            demand_kind,
            buffer_perf_class,
            cursor_color_fallback,
        ));
    }

    None
}

pub(super) fn apply_proposal_effect(
    proposal: InFlightProposal,
    buffer_handle: Option<i64>,
    requested_at: Millis,
) -> Effect {
    Effect::ApplyProposal(Box::new(ApplyProposalEffect {
        proposal,
        buffer_handle,
        requested_at,
    }))
}

pub(super) fn request_render_plan_effect(
    planning: RenderPlanningContext,
    proposal_id: ProposalId,
    render_decision: RenderDecision,
    animation_schedule: AnimationSchedule,
    requested_at: Millis,
) -> Effect {
    Effect::RequestRenderPlan(Box::new(RequestRenderPlanEffect {
        proposal_id,
        planning,
        render_decision,
        animation_schedule,
        requested_at,
    }))
}

pub(super) fn ingress_cursor_presentation_effect(
    state: &CoreState,
    request: Option<IngressCursorPresentationRequest>,
) -> Option<Effect> {
    let request = request?;
    let runtime = state.runtime();
    if !runtime.is_enabled()
        || runtime.is_animating()
        || !request.mode_allowed()
        || runtime.config.hide_target_hack
        || !request.outside_cmdline()
    {
        return None;
    }

    Some(Effect::ApplyIngressCursorPresentation(
        request
            .prepaint_cell()
            .map_or(IngressCursorPresentationEffect::HideCursor, |cell| {
                IngressCursorPresentationEffect::HideCursorAndPrepaint {
                    cell,
                    shape: request.prepaint_shape(),
                    zindex: runtime.config.windows_zindex,
                }
            }),
    ))
}

pub(super) fn enter_hot_cleanup_state(
    state: CoreState,
    observed_at: Millis,
) -> (CoreState, Vec<Effect>) {
    let cleanup = hot_cleanup_state(&state, observed_at);
    let next_state = state.with_render_cleanup(cleanup);
    if next_state.timers().active_token(TimerId::Cleanup).is_some() {
        return (next_state, Vec::new());
    }
    arm_render_cleanup_timer(next_state, observed_at)
}

pub(super) fn arm_render_cleanup_timer(
    state: CoreState,
    requested_at: Millis,
) -> (CoreState, Vec<Effect>) {
    let Some(deadline) = state.render_cleanup().next_deadline() else {
        return (state, Vec::new());
    };
    let delay = deadline.value().saturating_sub(requested_at.value()).max(1);
    let (scheduled_state, effect) = schedule_timer_with_delay(
        state,
        TimerId::Cleanup,
        delay_budget_from_ms(delay),
        requested_at,
    );
    (scheduled_state, vec![effect])
}

pub(super) fn advance_cleanup_for_proposal(
    state: CoreState,
    proposal: &InFlightProposal,
    observed_at: Millis,
) -> (CoreState, Vec<Effect>) {
    match proposal.cleanup_action() {
        RenderCleanupAction::NoAction => (state, Vec::new()),
        RenderCleanupAction::Invalidate => enter_hot_cleanup_state(state, observed_at),
        RenderCleanupAction::Schedule => {
            let cleanup = scheduled_cleanup_state(&state, observed_at);
            let next_state = state.with_render_cleanup(cleanup);
            if next_state.timers().active_token(TimerId::Cleanup).is_some() {
                return (next_state, Vec::new());
            }
            arm_render_cleanup_timer(next_state, observed_at)
        }
    }
}

pub(super) fn cleanup_effect_for_timer_fire(
    cleanup: RenderCleanupState,
    config: &crate::config::RuntimeConfig,
    observed_at: Millis,
) -> Option<Effect> {
    match cleanup {
        RenderCleanupState::Hot(schedule) => {
            if observed_at.value() >= schedule.hard_purge_due_at().value() {
                return Some(Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
                    execution: RenderCleanupExecution::HardPurge,
                }));
            }
            if observed_at.value() < schedule.next_compaction_due_at().value() {
                return None;
            }
            Some(Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
                execution: RenderCleanupExecution::SoftClear {
                    max_kept_windows: config.max_kept_windows,
                },
            }))
        }
        RenderCleanupState::Cooling(schedule) => {
            if observed_at.value() >= schedule.hard_purge_due_at().value() {
                return Some(Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
                    execution: RenderCleanupExecution::HardPurge,
                }));
            }
            if observed_at.value() < schedule.next_compaction_due_at().value() {
                return None;
            }
            Some(Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
                execution: RenderCleanupExecution::CompactToBudget {
                    target_budget: render_cleanup_idle_target_budget(config),
                    max_prune_per_tick: render_cleanup_max_prune_per_tick(config),
                },
            }))
        }
        RenderCleanupState::Cold => None,
    }
}

pub(super) fn cleanup_state_after_applied(
    cleanup: RenderCleanupState,
    action: RenderCleanupAppliedAction,
    observed_at: Millis,
) -> RenderCleanupState {
    match action {
        RenderCleanupAppliedAction::SoftCleared => cleanup.enter_cooling(observed_at),
        RenderCleanupAppliedAction::CompactedToBudget {
            converged_to_idle: true,
        } => cleanup.converge_to_cold(),
        RenderCleanupAppliedAction::CompactedToBudget {
            converged_to_idle: false,
        } => cleanup.continue_cooling(observed_at),
        RenderCleanupAppliedAction::HardPurged => cleanup.converge_to_cold(),
    }
}

pub(super) fn redraw_effect_for_proposal(
    proposal: &InFlightProposal,
    visual_change: bool,
) -> Option<Effect> {
    if !visual_change {
        return None;
    }

    match proposal.execution() {
        ProposalExecution::Draw { .. } if proposal.side_effects().redraw_after_draw_if_cmdline => {
            Some(Effect::RedrawCmdline)
        }
        ProposalExecution::Clear { .. } => {
            // hiding smear windows at settle/clear time must flush the UI even outside
            // cmdline mode or the old frame can remain visible until the next user input.
            Some(Effect::RedrawCmdline)
        }
        ProposalExecution::Draw { .. }
        | ProposalExecution::Noop { .. }
        | ProposalExecution::Failure { .. } => None,
    }
}

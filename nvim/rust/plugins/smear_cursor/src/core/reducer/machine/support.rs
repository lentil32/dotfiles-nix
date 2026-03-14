use crate::core::effect::{
    ApplyProposalEffect, ApplyRenderCleanupEffect, CursorPositionReadPolicy, Effect,
    EventLoopMetricEffect, IngressCursorPresentationEffect, IngressCursorPresentationRequest,
    ObservationRuntimeContext, RenderCleanupExecution, RequestObservationBaseEffect,
    RequestProbeEffect, RequestRenderPlanEffect, ScheduleTimerEffect, TimerKind,
};
use crate::core::event::RenderCleanupAppliedAction;
use crate::core::runtime_reducer::{
    RenderCleanupAction, RenderDecision, as_delay_ms, render_cleanup_delay_ms,
    render_hard_cleanup_delay_ms,
};
use crate::core::state::{
    CoreState, ExternalDemandKind, InFlightProposal, ObservationBasis, ObservationRequest,
    ObservationSnapshot, ProbeKind, ProbeRequestSet, ProbeState, RealizationPlan,
    RenderCleanupState,
};
use crate::core::types::{DelayBudgetMs, Millis, ProposalId, TimerId};

pub(super) const DEFAULT_ANIMATION_DELAY_MS: DelayBudgetMs = DelayBudgetMs::DEFAULT_ANIMATION;
pub(super) const RECOVERY_MAX_ATTEMPTS: u8 = 5;
pub(super) const DEFAULT_RECOVERY_DELAY_MS: u64 = 16;
const RECOVERY_MAX_DELAY_MS: u64 = 256;

pub(super) fn delay_budget_from_ms(delay_ms: u64) -> DelayBudgetMs {
    match DelayBudgetMs::try_new(delay_ms) {
        Ok(delay) => delay,
        // Surprising: recovery backoff produced a non-positive delay. Fall back to animation default.
        Err(_) => DEFAULT_ANIMATION_DELAY_MS,
    }
}

fn recovery_backoff_delay(attempt: u8) -> DelayBudgetMs {
    if attempt == 0 {
        return delay_budget_from_ms(DEFAULT_RECOVERY_DELAY_MS);
    }

    let shift = u32::from(attempt.saturating_sub(1));
    let scaled = match 1_u64.checked_shl(shift) {
        Some(scale) => DEFAULT_RECOVERY_DELAY_MS.saturating_mul(scale),
        None => RECOVERY_MAX_DELAY_MS,
    };
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
    let recovering = with_recovery_attempt(state.into_recovering(), attempt);
    let delay = recovery_backoff_delay(attempt);
    schedule_timer_with_delay(recovering, TimerKind::Recovery, delay, observed_at)
}

pub(super) fn ingress_marks_cursor_autocmd_freshness(kind: ExternalDemandKind) -> bool {
    matches!(
        kind,
        ExternalDemandKind::ExternalCursor | ExternalDemandKind::ModeChanged
    )
}

pub(super) fn suppresses_key_fallback_at(state: &CoreState, observed_at: Millis) -> bool {
    let freshness_window_ms = as_delay_ms(state.runtime().config.delay_after_key);
    !state
        .ingress_policy()
        .admits_key_fallback(observed_at, freshness_window_ms)
}

pub(super) fn schedule_timer_with_delay(
    state: CoreState,
    kind: TimerKind,
    delay: DelayBudgetMs,
    requested_at: Millis,
) -> (CoreState, Effect) {
    let timer_id = kind.timer_id();
    let (timers, token) = state.timers().arm(timer_id);
    let next_state = state.with_timers(timers);
    let effect = Effect::ScheduleTimer(ScheduleTimerEffect {
        kind,
        token,
        delay,
        requested_at,
    });
    (next_state, effect)
}

pub(super) fn delayed_pending_ingress_due_at(
    state: &CoreState,
    observed_at: Millis,
) -> Option<Millis> {
    let due_at = state.demand_queue().next_due_at()?;
    (due_at.value() > observed_at.value()).then_some(due_at)
}

pub(super) fn arm_delayed_ingress_wake(
    state: CoreState,
    due_at: Millis,
    observed_at: Millis,
) -> (CoreState, Effect) {
    let delay_ms = due_at.value().saturating_sub(observed_at.value());
    schedule_timer_with_delay(
        state,
        TimerKind::Ingress,
        delay_budget_from_ms(delay_ms),
        observed_at,
    )
}

fn cursor_position_read_policy(state: &CoreState) -> CursorPositionReadPolicy {
    CursorPositionReadPolicy::new(state.runtime().config.smear_to_cmd)
}

fn observation_runtime_context(state: &CoreState) -> ObservationRuntimeContext {
    ObservationRuntimeContext::new(
        cursor_position_read_policy(state),
        state.runtime().config.scroll_buffer_space,
        state.runtime().tracked_location(),
        state.runtime().current_corners(),
    )
}

pub(super) fn request_observation_base(state: &CoreState, request: ObservationRequest) -> Effect {
    Effect::RequestObservationBase(RequestObservationBaseEffect {
        request,
        context: observation_runtime_context(state),
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

fn request_probe(
    state: &CoreState,
    observation_basis: ObservationBasis,
    probe_request_id: crate::core::types::ProbeRequestId,
    kind: ProbeKind,
    background_chunk: Option<crate::core::state::BackgroundProbeChunk>,
) -> Effect {
    Effect::RequestProbe(RequestProbeEffect {
        observation_basis,
        probe_request_id,
        kind,
        cursor_position_policy: cursor_position_read_policy(state),
        background_chunk,
    })
}

pub(super) fn probe_requests_for(state: &CoreState) -> ProbeRequestSet {
    ProbeRequestSet::new(
        state.runtime().config.requires_cursor_color_sampling(),
        state.runtime().config.requires_background_sampling(),
    )
}

pub(super) fn next_pending_probe_effect(
    state: &CoreState,
    observation: &ObservationSnapshot,
) -> Option<Effect> {
    let basis = observation.basis();
    if let ProbeState::Pending { request_id } = observation.probes().cursor_color() {
        return Some(request_probe(
            state,
            basis.clone(),
            *request_id,
            ProbeKind::CursorColor,
            None,
        ));
    }

    if let ProbeState::Pending { request_id } = observation.probes().background() {
        return Some(request_probe(
            state,
            basis.clone(),
            *request_id,
            ProbeKind::Background,
            observation
                .background_progress()
                .and_then(crate::core::state::BackgroundProbeProgress::next_chunk),
        ));
    }

    None
}

pub(super) fn apply_proposal_effect(proposal: InFlightProposal, requested_at: Millis) -> Effect {
    Effect::ApplyProposal(Box::new(ApplyProposalEffect {
        proposal,
        requested_at,
    }))
}

pub(super) fn request_render_plan_effect(
    planning_state: CoreState,
    observation: ObservationSnapshot,
    proposal_id: ProposalId,
    render_decision: RenderDecision,
    should_schedule_next_animation: bool,
    next_animation_at_ms: Option<Millis>,
    requested_at: Millis,
) -> Effect {
    Effect::RequestRenderPlan(Box::new(RequestRenderPlanEffect {
        proposal_id,
        planning_state,
        observation,
        render_decision,
        should_schedule_next_animation,
        next_animation_at_ms,
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
        match request.prepaint_cell() {
            Some(cell) => IngressCursorPresentationEffect::HideCursorAndPrepaint {
                cell,
                zindex: runtime.config.windows_zindex,
            },
            None => IngressCursorPresentationEffect::HideCursor,
        },
    ))
}

fn proposal_max_kept_windows(state: &CoreState, proposal: &InFlightProposal) -> usize {
    match proposal.realization() {
        RealizationPlan::Draw(draw) => draw.max_kept_windows(),
        RealizationPlan::Clear(clear) => clear.max_kept_windows(),
        RealizationPlan::Noop | RealizationPlan::Failure(_) => {
            state.runtime().config.max_kept_windows
        }
    }
}

pub(super) fn invalidate_cleanup_state(state: CoreState) -> CoreState {
    let timers = state.timers().clear_active(TimerId::Cleanup);
    state
        .with_timers(timers)
        .with_render_cleanup(RenderCleanupState::Inactive)
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
        TimerKind::Cleanup,
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
        RenderCleanupAction::Invalidate => (invalidate_cleanup_state(state), Vec::new()),
        RenderCleanupAction::Schedule => {
            let cleanup = RenderCleanupState::scheduled(
                observed_at,
                render_cleanup_delay_ms(&state.runtime().config),
                render_hard_cleanup_delay_ms(&state.runtime().config),
                proposal_max_kept_windows(&state, proposal),
            );
            arm_render_cleanup_timer(state.with_render_cleanup(cleanup), observed_at)
        }
    }
}

pub(super) fn cleanup_effect_for_timer_fire(
    cleanup: RenderCleanupState,
    observed_at: Millis,
) -> Option<Effect> {
    match cleanup {
        RenderCleanupState::Inactive => None,
        RenderCleanupState::Armed {
            soft_due_at,
            hard_due_at: _,
            ..
        } if observed_at.value() < soft_due_at.value() => None,
        RenderCleanupState::Armed { hard_due_at, .. }
        | RenderCleanupState::SoftCleared { hard_due_at, .. }
            if observed_at.value() >= hard_due_at.value() =>
        {
            Some(Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
                execution: RenderCleanupExecution::HardPurge,
            }))
        }
        RenderCleanupState::Armed {
            max_kept_windows, ..
        } => Some(Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
            execution: RenderCleanupExecution::SoftClear { max_kept_windows },
        })),
        RenderCleanupState::SoftCleared { .. } => None,
    }
}

pub(super) fn cleanup_state_after_applied(
    cleanup: RenderCleanupState,
    action: RenderCleanupAppliedAction,
) -> RenderCleanupState {
    match action {
        RenderCleanupAppliedAction::SoftCleared => cleanup.soft_cleared(),
        RenderCleanupAppliedAction::HardPurged => RenderCleanupState::Inactive,
    }
}

pub(super) fn redraw_effect_for_proposal(
    proposal: &InFlightProposal,
    visual_change: bool,
) -> Option<Effect> {
    if !visual_change {
        return None;
    }

    match proposal.realization() {
        RealizationPlan::Draw(_) if proposal.side_effects().redraw_after_draw_if_cmdline => {
            Some(Effect::RedrawCmdline)
        }
        RealizationPlan::Clear(_) => {
            // Comment: hiding smear windows at settle/clear time must flush the UI even outside
            // cmdline mode or the old frame can remain visible until the next user input.
            Some(Effect::RedrawCmdline)
        }
        _ => None,
    }
}

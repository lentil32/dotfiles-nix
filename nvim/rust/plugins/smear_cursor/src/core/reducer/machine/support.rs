use crate::core::effect::{
    ApplyProposalEffect, ApplyRenderCleanupEffect, CursorPositionReadPolicy, Effect,
    EventLoopMetricEffect, IngressCursorPresentationEffect, IngressCursorPresentationRequest,
    ObservationRuntimeContext, RenderCleanupExecution, RequestObservationBaseEffect,
    RequestProbeEffect, RequestRenderPlanEffect, ScheduleTimerEffect, TimerKind,
};
use crate::core::event::RenderCleanupAppliedAction;
use crate::core::runtime_reducer::{
    RenderCleanupAction, RenderDecision, render_cleanup_delay_ms, render_hard_cleanup_delay_ms,
};
use crate::core::state::{
    AnimationSchedule, CoreState, ExternalDemandKind, InFlightProposal, ObservationBasis,
    ObservationRequest, ObservationSnapshot, ProbeKind, ProbeRequestSet, ProbeState,
    ProposalExecution, RenderCleanupState,
};
use crate::core::types::{DelayBudgetMs, Millis, ProposalId, TimerId};

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

fn hot_cleanup_state(
    state: &CoreState,
    observed_at: Millis,
    max_kept_windows: usize,
) -> RenderCleanupState {
    RenderCleanupState::scheduled(
        observed_at,
        render_cleanup_delay_ms(&state.runtime().config),
        render_hard_cleanup_delay_ms(&state.runtime().config),
        max_kept_windows,
    )
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
        token,
        delay,
        requested_at,
    });
    (next_state, effect)
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
    if let crate::core::state::ProbeSlot::Requested(ProbeState::Pending { request_id }) =
        observation.probes().cursor_color()
    {
        return Some(request_probe(
            state,
            basis.clone(),
            *request_id,
            ProbeKind::CursorColor,
            None,
        ));
    }

    if let crate::core::state::ProbeSlot::Requested(ProbeState::Pending { request_id }) =
        observation.probes().background()
    {
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
    animation_schedule: AnimationSchedule,
    requested_at: Millis,
) -> Effect {
    Effect::RequestRenderPlan(Box::new(RequestRenderPlanEffect {
        proposal_id,
        planning_state,
        observation,
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
                    zindex: runtime.config.windows_zindex,
                }
            }),
    ))
}

fn proposal_max_kept_windows(state: &CoreState, proposal: &InFlightProposal) -> usize {
    match proposal.execution() {
        ProposalExecution::Draw {
            realization: draw, ..
        } => draw.max_kept_windows(),
        ProposalExecution::Clear {
            realization: clear, ..
        } => clear.max_kept_windows(),
        ProposalExecution::Noop { .. } | ProposalExecution::Failure { .. } => {
            state.runtime().config.max_kept_windows
        }
    }
}

pub(super) fn enter_hot_cleanup_state(
    state: CoreState,
    observed_at: Millis,
    max_kept_windows: usize,
) -> (CoreState, Vec<Effect>) {
    let cleanup = hot_cleanup_state(&state, observed_at, max_kept_windows);
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
        RenderCleanupAction::Invalidate => {
            let max_kept_windows = proposal_max_kept_windows(&state, proposal);
            enter_hot_cleanup_state(state, observed_at, max_kept_windows)
        }
        RenderCleanupAction::Schedule => {
            let cleanup = hot_cleanup_state(
                &state,
                observed_at,
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
    if cleanup
        .hard_purge_due_at()
        .is_some_and(|hard_due_at| observed_at.value() >= hard_due_at.value())
    {
        return Some(Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
            execution: RenderCleanupExecution::HardPurge,
        }));
    }

    match cleanup.thermal() {
        crate::core::state::RenderThermalState::Hot => {
            let next_compaction_due_at = cleanup.next_compaction_due_at()?;
            if observed_at.value() < next_compaction_due_at.value() {
                return None;
            }
            Some(Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
                execution: RenderCleanupExecution::SoftClear {
                    max_kept_windows: cleanup.max_kept_windows(),
                },
            }))
        }
        crate::core::state::RenderThermalState::Cooling => {
            let next_compaction_due_at = cleanup.next_compaction_due_at()?;
            if observed_at.value() < next_compaction_due_at.value() {
                return None;
            }
            Some(Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
                execution: RenderCleanupExecution::CompactToBudget {
                    target_budget: cleanup.idle_target_budget(),
                    max_prune_per_tick: cleanup.max_prune_per_tick(),
                },
            }))
        }
        crate::core::state::RenderThermalState::Cold => None,
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

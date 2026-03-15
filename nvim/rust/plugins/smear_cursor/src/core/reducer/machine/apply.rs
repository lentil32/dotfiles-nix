use super::Transition;
use super::observation::start_next_observation;
use super::support::{
    DEFAULT_ANIMATION_DELAY_MS, advance_cleanup_for_proposal, apply_proposal_effect,
    arm_delayed_ingress_wake, arm_render_cleanup_timer, cleanup_state_after_applied,
    delay_budget_from_ms, delayed_pending_ingress_due_at, enter_recovering_with_backoff,
    record_event_loop_metric, redraw_effect_for_proposal, schedule_timer_with_delay,
};
use crate::core::effect::{EventLoopMetricEffect, TimerKind};
use crate::core::event::{
    ApplyReport, EffectFailedEvent, RenderCleanupAppliedEvent, RenderPlanComputedEvent,
    RenderPlanFailedEvent,
};
use crate::core::state::{CoreState, InFlightProposal, RealizationDivergence, RealizationLedger};
use crate::core::types::{Millis, ProposalId};

fn take_in_flight_proposal(
    state: CoreState,
    proposal_id: ProposalId,
) -> Option<(CoreState, InFlightProposal)> {
    state.clear_pending_for(proposal_id)
}

pub(super) fn reduce_render_plan_computed(
    state: &CoreState,
    payload: RenderPlanComputedEvent,
) -> Transition {
    if state.pending_plan_proposal_id() != Some(payload.proposal_id) {
        return Transition::new(
            state.clone(),
            vec![record_event_loop_metric(EventLoopMetricEffect::StaleToken)],
        );
    }

    let proposal = payload.planned_render.proposal().clone();
    let Some(next_state) = state.clone().accept_planned_render(*payload.planned_render) else {
        return Transition::new(
            state.clone(),
            vec![record_event_loop_metric(EventLoopMetricEffect::StaleToken)],
        );
    };

    Transition::new(
        next_state,
        vec![apply_proposal_effect(proposal, payload.observed_at)],
    )
}

pub(super) fn reduce_render_plan_failed(
    state: &CoreState,
    payload: RenderPlanFailedEvent,
) -> Transition {
    if state.pending_plan_proposal_id() != Some(payload.proposal_id) {
        return Transition::new(
            state.clone(),
            vec![record_event_loop_metric(EventLoopMetricEffect::StaleToken)],
        );
    }

    let (next_state, schedule) = enter_recovering_with_backoff(state.clone(), payload.observed_at);
    Transition::new(next_state, vec![schedule])
}

pub(super) fn reduce_apply_reported(state: &CoreState, payload: ApplyReport) -> Transition {
    match payload {
        ApplyReport::AppliedFully {
            proposal_id,
            observed_at,
            visual_change,
        } => reduce_apply_completed(state, proposal_id, observed_at, visual_change),
        ApplyReport::AppliedDegraded {
            proposal_id,
            divergence,
            observed_at,
            visual_change,
        } => reduce_apply_non_success(state, proposal_id, divergence, observed_at, visual_change),
        ApplyReport::ApplyFailed {
            proposal_id,
            divergence,
            observed_at,
            ..
        } => reduce_apply_non_success(state, proposal_id, divergence, observed_at, false),
    }
}

pub(super) fn reduce_render_cleanup_applied(
    state: &CoreState,
    payload: RenderCleanupAppliedEvent,
) -> Transition {
    if state.needs_initialize() {
        return Transition::stay(state);
    }

    let next_state = state
        .clone()
        .with_realization(state.realization().clone().cleanup_applied())
        .with_render_cleanup(cleanup_state_after_applied(
            state.render_cleanup(),
            payload.action,
        ));
    let (next_state, effects) = arm_render_cleanup_timer(next_state, payload.observed_at);
    Transition::new(next_state, effects)
}

fn reduce_apply_completed(
    state: &CoreState,
    proposal_id: ProposalId,
    observed_at: Millis,
    visual_change: bool,
) -> Transition {
    let Some((cleared_state, proposal)) = take_in_flight_proposal(state.clone(), proposal_id)
    else {
        return Transition::new(
            state.clone(),
            vec![record_event_loop_metric(EventLoopMetricEffect::StaleToken)],
        );
    };
    let ready_state = cleared_state.with_realization(RealizationLedger::acknowledge(
        proposal.basis().target().cloned(),
    ));

    let (ready_state, mut effects) =
        advance_cleanup_for_proposal(ready_state, &proposal, observed_at);
    if let Some(effect) = redraw_effect_for_proposal(&proposal, visual_change) {
        effects.push(effect);
    }

    let (base_state, dispatch) = start_next_observation(ready_state, observed_at);
    let dispatch_present = dispatch.is_some();
    let mut next_state = base_state;
    effects.extend(dispatch);

    match proposal.animation_schedule() {
        crate::core::state::AnimationSchedule::Idle => {}
        crate::core::state::AnimationSchedule::DefaultDelay
        | crate::core::state::AnimationSchedule::Deadline(_) => {
            let animation_delay = proposal.animation_schedule().deadline().map_or(
                DEFAULT_ANIMATION_DELAY_MS,
                |deadline| {
                    let remaining = deadline.value().saturating_sub(observed_at.value()).max(1);
                    delay_budget_from_ms(remaining)
                },
            );
            let (scheduled_state, schedule) = schedule_timer_with_delay(
                next_state,
                TimerKind::Animation,
                animation_delay,
                observed_at,
            );
            next_state = scheduled_state;
            effects.push(schedule);
        }
    }

    if !dispatch_present
        && matches!(
            next_state.lifecycle(),
            crate::core::types::Lifecycle::Primed | crate::core::types::Lifecycle::Ready
        )
        && let Some(due_at) = delayed_pending_ingress_due_at(&next_state, observed_at)
    {
        let base_state = next_state;
        let (scheduled_state, wake) = arm_delayed_ingress_wake(base_state, due_at, observed_at);
        next_state = scheduled_state;
        effects.push(wake);
    }

    Transition::new(next_state, effects)
}

fn reduce_apply_non_success(
    state: &CoreState,
    proposal_id: ProposalId,
    divergence: RealizationDivergence,
    observed_at: Millis,
    visual_change: bool,
) -> Transition {
    let Some((cleared_state, proposal)) = take_in_flight_proposal(state.clone(), proposal_id)
    else {
        return Transition::new(
            state.clone(),
            vec![record_event_loop_metric(EventLoopMetricEffect::StaleToken)],
        );
    };
    let realized = cleared_state.with_realization(RealizationLedger::diverged_from(
        proposal.basis().acknowledged().cloned(),
        divergence,
    ));

    let (realized, mut effects) = advance_cleanup_for_proposal(realized, &proposal, observed_at);
    if let Some(effect) = redraw_effect_for_proposal(&proposal, visual_change) {
        effects.push(effect);
    }

    let (next_state, schedule) = enter_recovering_with_backoff(realized, observed_at);
    effects.push(schedule);
    Transition::new(next_state, effects)
}

pub(super) fn reduce_effect_failed(state: &CoreState, payload: EffectFailedEvent) -> Transition {
    if state.needs_initialize() {
        // Surprising: boundary failures can surface before core initialization.
        // Keep state stable until the first Initialize event establishes observation.
        return Transition::stay(state);
    }

    if let Some(proposal_id) = payload.proposal_id {
        return reduce_apply_non_success(
            state,
            proposal_id,
            RealizationDivergence::ShellStateUnknown,
            payload.observed_at,
            false,
        );
    }

    let (next_state, schedule) = enter_recovering_with_backoff(state.clone(), payload.observed_at);
    Transition::new(next_state, vec![schedule])
}

use super::Transition;
use super::observation::start_next_observation;
use super::support::DEFAULT_ANIMATION_DELAY_MS;
use super::support::advance_cleanup_for_proposal;
use super::support::apply_proposal_effect;
use super::support::arm_render_cleanup_timer;
use super::support::cleanup_state_after_applied;
use super::support::delay_budget_from_ms;
use super::support::enter_recovering_with_backoff;
use super::support::exact_boundary_refresh_required;
use super::support::record_event_loop_metric;
use super::support::redraw_effect_for_proposal;
use super::support::schedule_timer_with_delay;
use super::support::start_boundary_refresh_observation;
use crate::core::effect::ApplyRenderCleanupEffect;
use crate::core::effect::EventLoopMetricEffect;
use crate::core::effect::RenderCleanupExecution;
use crate::core::effect::TimerKind;
use crate::core::event::ApplyReport;
use crate::core::event::EffectFailedEvent;
use crate::core::event::RenderCleanupAppliedEvent;
use crate::core::event::RenderPlanComputedEvent;
use crate::core::event::RenderPlanFailedEvent;
use crate::core::state::CoreState;
use crate::core::state::RealizationDivergence;
use crate::core::state::RealizationLedger;
use crate::core::types::Millis;
use crate::core::types::ProposalId;

pub(super) fn reduce_render_plan_computed(
    mut state: CoreState,
    payload: RenderPlanComputedEvent,
) -> Transition {
    if state.pending_plan_proposal_id() != Some(payload.proposal_id) {
        return Transition::new(
            state,
            vec![record_event_loop_metric(EventLoopMetricEffect::StaleToken)],
        );
    }

    let planned_render = *payload.planned_render;
    let proposal = planned_render.proposal().clone();
    let buffer_handle = state
        .observation()
        .map(|observation| observation.basis().cursor_location().buffer_handle);
    if !state.accept_planned_render_mut(planned_render) {
        return Transition::new(
            state,
            vec![record_event_loop_metric(EventLoopMetricEffect::StaleToken)],
        );
    }

    Transition::new(
        state,
        vec![apply_proposal_effect(
            proposal,
            buffer_handle,
            payload.observed_at,
        )],
    )
}

pub(super) fn reduce_render_plan_failed(
    state: CoreState,
    payload: RenderPlanFailedEvent,
) -> Transition {
    if state.pending_plan_proposal_id() != Some(payload.proposal_id) {
        return Transition::new(
            state,
            vec![record_event_loop_metric(EventLoopMetricEffect::StaleToken)],
        );
    }

    let (next_state, schedule) = enter_recovering_with_backoff(state, payload.observed_at);
    Transition::new(next_state, vec![schedule])
}

pub(super) fn reduce_apply_reported(state: CoreState, payload: ApplyReport) -> Transition {
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
    state: CoreState,
    payload: RenderCleanupAppliedEvent,
) -> Transition {
    if state.needs_initialize() {
        return Transition::stay_owned(state);
    }

    let previous_cleanup = state.render_cleanup();
    let next_cleanup =
        cleanup_state_after_applied(previous_cleanup, payload.action, payload.observed_at);
    let next_realization = state.realization().clone().cleanup_applied();
    let next_state = state
        .with_realization(next_realization)
        .with_render_cleanup(next_cleanup);
    if matches!(
        payload.action,
        crate::core::event::RenderCleanupAppliedAction::SoftCleared
    ) && next_cleanup
        .next_compaction_due_at()
        .is_some_and(|due_at| due_at.value() <= payload.observed_at.value())
    {
        return Transition::new(
            next_state,
            vec![crate::core::effect::Effect::ApplyRenderCleanup(
                ApplyRenderCleanupEffect {
                    execution: RenderCleanupExecution::CompactToBudget {
                        target_budget: next_cleanup.idle_target_budget(),
                        max_prune_per_tick: next_cleanup.max_prune_per_tick(),
                    },
                },
            )],
        );
    }

    let (next_state, mut effects) = arm_render_cleanup_timer(next_state, payload.observed_at);
    if matches!(
        (previous_cleanup.thermal(), next_cleanup.thermal()),
        (
            crate::core::state::RenderThermalState::Cooling,
            crate::core::state::RenderThermalState::Cold
        )
    ) && let Some(started_at) = previous_cleanup.entered_cooling_at()
    {
        effects.push(record_event_loop_metric(
            EventLoopMetricEffect::CleanupConvergedToCold {
                started_at,
                converged_at: payload.observed_at,
            },
        ));
    }
    Transition::new(next_state, effects)
}

fn reduce_apply_completed(
    mut state: CoreState,
    proposal_id: ProposalId,
    observed_at: Millis,
    visual_change: bool,
) -> Transition {
    let Some(proposal) = state.take_pending_proposal(proposal_id) else {
        return Transition::new(
            state,
            vec![record_event_loop_metric(EventLoopMetricEffect::StaleToken)],
        );
    };
    let ready_state = state.with_realization(RealizationLedger::acknowledge(
        proposal.basis().target().cloned(),
    ));

    let (ready_state, mut effects) =
        advance_cleanup_for_proposal(ready_state, &proposal, observed_at);
    if let Some(effect) = redraw_effect_for_proposal(&proposal, visual_change) {
        effects.push(effect);
    }

    let (base_state, dispatch) = start_next_observation(ready_state);
    let dispatched_observation = dispatch.is_some();
    let mut next_state = base_state;
    effects.extend(dispatch);
    if !dispatched_observation
        && proposal.animation_schedule() == crate::core::state::AnimationSchedule::Idle
        && exact_boundary_refresh_required(&next_state)
    {
        let Some((refresh_state, refresh_effect)) =
            start_boundary_refresh_observation(next_state, observed_at)
        else {
            unreachable!("boundary refresh eligibility changed between check and start");
        };
        next_state = refresh_state;
        effects.push(refresh_effect);
    }

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

    Transition::new(next_state, effects)
}

fn reduce_apply_non_success(
    mut state: CoreState,
    proposal_id: ProposalId,
    divergence: RealizationDivergence,
    observed_at: Millis,
    visual_change: bool,
) -> Transition {
    let Some(proposal) = state.take_pending_proposal(proposal_id) else {
        return Transition::new(
            state,
            vec![record_event_loop_metric(EventLoopMetricEffect::StaleToken)],
        );
    };
    let realized = state.with_realization(RealizationLedger::diverged_from(
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

pub(super) fn reduce_effect_failed(state: CoreState, payload: EffectFailedEvent) -> Transition {
    if state.needs_initialize() {
        // Surprising: boundary failures can surface before core initialization.
        // Keep state stable until the first Initialize event establishes observation.
        return Transition::stay_owned(state);
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

    let (next_state, schedule) = enter_recovering_with_backoff(state, payload.observed_at);
    Transition::new(next_state, vec![schedule])
}

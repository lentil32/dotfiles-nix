use super::Transition;
use super::observation::{observe_or_plan, transition_ready_or_observe};
use super::support::{
    arm_render_cleanup_timer, cleanup_effect_for_timer_fire, delay_budget_from_ms,
    record_event_loop_metric, reset_recovery_attempt, schedule_timer_with_delay,
};
use crate::core::effect::{EventLoopMetricEffect, TimerKind};
use crate::core::event::{TimerFiredWithTokenEvent, TimerLostWithTokenEvent};
use crate::core::state::CoreState;
use crate::core::types::{Millis, TimerToken};

fn reduce_ingress_timer_signal(state: CoreState, observed_at: Millis) -> Transition {
    let Some(pending_delay_until) = state.ingress_policy().pending_delay_until() else {
        return Transition::new(state, Vec::new());
    };
    if !matches!(
        state.lifecycle(),
        crate::core::types::Lifecycle::Primed | crate::core::types::Lifecycle::Ready
    ) {
        // Surprising: delayed ingress survived into a non-ready lifecycle. Clear the reducer truth
        // and let the already-fired host timer collapse to a no-op.
        let cleared_policy = state.ingress_policy().clear_pending_delay();
        let cleared_state = state.with_ingress_policy(cleared_policy);
        return Transition::new(cleared_state, Vec::new());
    }
    if observed_at.value() < pending_delay_until.value() {
        let remaining_delay_ms = pending_delay_until
            .value()
            .saturating_sub(observed_at.value())
            .max(1);
        let (scheduled_state, effect) = schedule_timer_with_delay(
            state,
            TimerKind::Ingress,
            delay_budget_from_ms(remaining_delay_ms),
            observed_at,
        );
        return Transition::new(scheduled_state, vec![effect]);
    }

    let cleared_policy = state.ingress_policy().clear_pending_delay();
    let cleared_state = state.with_ingress_policy(cleared_policy);
    transition_ready_or_observe(cleared_state, observed_at)
}

fn reduce_timer_signal_with_token(
    state: &CoreState,
    token: TimerToken,
    observed_at: Millis,
) -> Transition {
    let kind = TimerKind::from_timer_id(token.id());
    if !state.timers().is_active(token) {
        if kind == TimerKind::Cleanup {
            return Transition::stay(state);
        }
        return Transition::new(
            state.clone(),
            vec![record_event_loop_metric(EventLoopMetricEffect::StaleToken)],
        );
    }

    let disarmed_state = state
        .clone()
        .with_timers(state.timers().clear_matching(token));
    let transition = match kind {
        TimerKind::Animation => {
            if disarmed_state.needs_initialize() {
                Transition::stay(&disarmed_state)
            } else {
                observe_or_plan(disarmed_state, observed_at)
            }
        }
        TimerKind::Ingress => reduce_ingress_timer_signal(disarmed_state, observed_at),
        TimerKind::Recovery => {
            if disarmed_state.lifecycle() != crate::core::types::Lifecycle::Recovering {
                Transition::stay(&disarmed_state)
            } else {
                let settled = match disarmed_state.retained_observation().cloned() {
                    Some(observation) => reset_recovery_attempt(
                        disarmed_state.into_ready_with_observation(observation),
                    ),
                    None => reset_recovery_attempt(disarmed_state.into_primed()),
                };
                observe_or_plan(settled, observed_at)
            }
        }
        TimerKind::Cleanup => {
            match cleanup_effect_for_timer_fire(disarmed_state.render_cleanup(), observed_at) {
                Some(effect) => Transition::new(disarmed_state, vec![effect]),
                None => {
                    let (scheduled_state, effects) =
                        arm_render_cleanup_timer(disarmed_state, observed_at);
                    Transition::new(scheduled_state, effects)
                }
            }
        }
    };

    transition
}

pub(super) fn reduce_timer_fired_with_token(
    state: &CoreState,
    payload: TimerFiredWithTokenEvent,
) -> Transition {
    reduce_timer_signal_with_token(state, payload.token, payload.observed_at)
}

pub(super) fn reduce_timer_lost_with_token(
    state: &CoreState,
    payload: TimerLostWithTokenEvent,
) -> Transition {
    reduce_timer_signal_with_token(state, payload.token, payload.observed_at)
}

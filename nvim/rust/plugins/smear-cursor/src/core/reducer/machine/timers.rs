use super::Transition;
use super::observation::observe_or_plan;
use super::observation::start_next_observation;
use super::observation::transition_ready_or_observe;
use super::support::arm_render_cleanup_timer;
use super::support::cleanup_effect_for_timer_fire;
use super::support::delay_budget_from_ms;
use super::support::exact_boundary_refresh_required;
use super::support::record_event_loop_metric;
use super::support::reset_recovery_attempt;
use super::support::schedule_timer_with_delay;
use super::support::start_boundary_refresh_observation;
use crate::core::effect::EventLoopMetricEffect;
use crate::core::effect::TimerKind;
use crate::core::event::TimerFiredWithTokenEvent;
use crate::core::event::TimerLostWithTokenEvent;
use crate::core::state::CoreState;
use crate::core::types::Millis;
use crate::core::types::TimerToken;

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
    state: CoreState,
    token: TimerToken,
    observed_at: Millis,
) -> Transition {
    let kind = TimerKind::from_timer_id(token.id());
    if !state.timers().is_active(token) {
        if kind == TimerKind::Cleanup {
            return Transition::stay_owned(state);
        }
        return Transition::new(
            state,
            vec![record_event_loop_metric(EventLoopMetricEffect::StaleToken)],
        );
    }

    let next_timers = state.timers().clear_matching(token);
    let disarmed_state = state.with_timers(next_timers);
    match kind {
        TimerKind::Animation => {
            if disarmed_state.needs_initialize() {
                Transition::stay_owned(disarmed_state)
            } else {
                let (next_state, effect) = start_next_observation(disarmed_state, observed_at);
                if let Some(effect) = effect {
                    Transition::new(next_state, vec![effect])
                } else if exact_boundary_refresh_required(&next_state) {
                    let Some((refresh_state, refresh_effect)) =
                        start_boundary_refresh_observation(next_state, observed_at)
                    else {
                        unreachable!(
                            "boundary refresh eligibility changed between check and start"
                        );
                    };
                    Transition::new(refresh_state, vec![refresh_effect])
                } else {
                    observe_or_plan(next_state, observed_at)
                }
            }
        }
        TimerKind::Ingress => reduce_ingress_timer_signal(disarmed_state, observed_at),
        TimerKind::Recovery => {
            if disarmed_state.lifecycle() != crate::core::types::Lifecycle::Recovering {
                Transition::stay_owned(disarmed_state)
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
    }
}

pub(super) fn reduce_timer_fired_with_token(
    state: CoreState,
    payload: TimerFiredWithTokenEvent,
) -> Transition {
    reduce_timer_signal_with_token(state, payload.token, payload.observed_at)
}

pub(super) fn reduce_timer_lost_with_token(
    state: CoreState,
    payload: TimerLostWithTokenEvent,
) -> Transition {
    reduce_timer_signal_with_token(state, payload.token, payload.observed_at)
}

use super::Transition;
use super::observation::{observe_or_plan, transition_ready_or_observe};
use super::support::{
    arm_render_cleanup_timer, cleanup_effect_for_timer_fire, record_event_loop_metric,
    reset_recovery_attempt,
};
use crate::core::effect::{EventLoopMetricEffect, TimerKind};
use crate::core::event::{TimerFiredWithTokenEvent, TimerLostWithTokenEvent};
use crate::core::state::CoreState;
use crate::core::types::{Millis, TimerToken};

fn reduce_timer_signal_with_token(
    state: &CoreState,
    token: TimerToken,
    observed_at: Millis,
    timer_lost_fallback: bool,
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
        TimerKind::Ingress => match disarmed_state.lifecycle() {
            crate::core::types::Lifecycle::Primed | crate::core::types::Lifecycle::Ready => {
                transition_ready_or_observe(disarmed_state, observed_at)
            }
            crate::core::types::Lifecycle::Idle
            | crate::core::types::Lifecycle::Observing
            | crate::core::types::Lifecycle::Planning
            | crate::core::types::Lifecycle::Applying
            | crate::core::types::Lifecycle::Recovering => {
                Transition::new(disarmed_state, Vec::new())
            }
        },
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

    let _timer_lost_fallback = timer_lost_fallback;
    transition
}

pub(super) fn reduce_timer_fired_with_token(
    state: &CoreState,
    payload: TimerFiredWithTokenEvent,
) -> Transition {
    reduce_timer_signal_with_token(state, payload.token, payload.observed_at, false)
}

pub(super) fn reduce_timer_lost_with_token(
    state: &CoreState,
    payload: TimerLostWithTokenEvent,
) -> Transition {
    reduce_timer_signal_with_token(state, payload.token, payload.observed_at, true)
}

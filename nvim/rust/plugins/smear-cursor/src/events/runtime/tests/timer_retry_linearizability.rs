use super::host_callback_id;
use super::host_timer_id;
use crate::core::effect::Effect;
use crate::core::effect::EventLoopMetricEffect;
use crate::core::event::Event;
use crate::core::event::TimerFiredWithTokenEvent;
use crate::core::reducer::reduce;
use crate::core::state::CoreState;
use crate::core::types::Millis;
use crate::core::types::TimerId;
use crate::core::types::TimerToken;
use crate::events::runtime::CoreTimerHandle;
use crate::events::runtime::timer_bridge::FiredCoreTimerLookup;
use crate::events::runtime::timer_bridge::TimerBridge;
use crate::events::runtime::timer_bridge::TimerRetryTransition;
use crate::events::timer_protocol::FiredHostTimer;
use pretty_assertions::assert_eq as pretty_assert_eq;

#[derive(Debug, Clone, PartialEq)]
struct TimerRetryModelObservation {
    emitted_events: Vec<Event>,
    pending_retry_count: usize,
    active_timer_after_duplicate: Option<TimerToken>,
    duplicate_reducer_effects: Vec<Effect>,
}

#[derive(Debug, Clone, PartialEq)]
struct HostCallbackNoiseObservation {
    stale_lookup: FiredCoreTimerLookup,
    active_timer_after_stale_callback: bool,
    mismatched_lookup: FiredCoreTimerLookup,
    active_timer_after_mismatched_callback: bool,
    fresh_lookup: FiredCoreTimerLookup,
    stale_reducer_effects: Vec<Effect>,
    active_timer_after_stale_reducer_event: Option<TimerToken>,
    active_timer_after_fresh_callback: bool,
    active_timer_after_fresh_reducer_event: Option<TimerToken>,
}

fn core_timer_handle(
    host_callback_id_value: i64,
    host_timer_id_value: i64,
    token: TimerToken,
) -> CoreTimerHandle {
    CoreTimerHandle {
        host_callback_id: host_callback_id(host_callback_id_value),
        host_timer_id: host_timer_id(host_timer_id_value),
        token,
    }
}

fn fired_host_timer(handle: CoreTimerHandle) -> FiredHostTimer {
    FiredHostTimer::new(handle.host_callback_id, handle.host_timer_id)
}

fn timer_fired_event(token: TimerToken, observed_at: Millis) -> Event {
    Event::TimerFiredWithToken(TimerFiredWithTokenEvent { token, observed_at })
}

fn semantic_timer_event_from_lookup(
    lookup: FiredCoreTimerLookup,
    observed_at: Millis,
) -> Option<Event> {
    match lookup {
        FiredCoreTimerLookup::Matched(handle) => Some(timer_fired_event(handle.token, observed_at)),
        FiredCoreTimerLookup::MismatchedHostTimerId { .. }
        | FiredCoreTimerLookup::MissingHandle => None,
    }
}

fn state_with_active_timer(timer_id: TimerId) -> (CoreState, TimerToken) {
    let state = CoreState::default();
    let (timers, token) = state.timers().arm(timer_id);
    (state.with_timers(timers), token)
}

#[test]
fn coalesced_reentry_retry_emits_one_semantic_timer_event_for_valid_reducer_token() {
    let (state, token) = state_with_active_timer(TimerId::Animation);
    let handle = core_timer_handle(
        /*host_callback_id_value*/ 11, /*host_timer_id_value*/ 17, token,
    );
    let fired_timer = fired_host_timer(handle);
    let observed_at = Millis::new(/*value*/ 101);
    let mut bridge = TimerBridge::default();

    pretty_assert_eq!(bridge.replace_handle(handle), None);
    pretty_assert_eq!(
        bridge.stage_retry(fired_timer),
        TimerRetryTransition::Staged
    );
    pretty_assert_eq!(
        bridge.stage_retry(fired_timer),
        TimerRetryTransition::Coalesced
    );
    bridge.release_retry(fired_timer);

    let emitted_events = [
        semantic_timer_event_from_lookup(bridge.resolve_fired(fired_timer), observed_at),
        semantic_timer_event_from_lookup(bridge.resolve_fired(fired_timer), observed_at),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    let first_transition = reduce(
        &state,
        emitted_events
            .first()
            .cloned()
            .expect("coalesced retry should emit the valid timer event once"),
    );
    let duplicate_transition = reduce(
        &first_transition.next,
        timer_fired_event(token, observed_at),
    );

    pretty_assert_eq!(
        TimerRetryModelObservation {
            emitted_events,
            pending_retry_count: bridge.pending_retry_len(),
            active_timer_after_duplicate: duplicate_transition
                .next
                .timers()
                .active_token(TimerId::Animation),
            duplicate_reducer_effects: duplicate_transition.effects,
        },
        TimerRetryModelObservation {
            emitted_events: vec![timer_fired_event(token, observed_at)],
            pending_retry_count: 0,
            active_timer_after_duplicate: None,
            duplicate_reducer_effects: vec![Effect::RecordEventLoopMetric(
                EventLoopMetricEffect::StaleToken
            )],
        }
    );
}

#[test]
fn stale_and_mismatched_host_callbacks_never_resurrect_or_consume_reducer_timer() {
    let (state, stale_token) = state_with_active_timer(TimerId::Recovery);
    let (timers, fresh_token) = state.timers().arm(TimerId::Recovery);
    let state = state.with_timers(timers);
    let stale_handle = core_timer_handle(
        /*host_callback_id_value*/ 19,
        /*host_timer_id_value*/ 23,
        stale_token,
    );
    let fresh_handle = core_timer_handle(
        /*host_callback_id_value*/ 29,
        /*host_timer_id_value*/ 31,
        fresh_token,
    );
    let mismatched_host_timer_id = host_timer_id(/*value*/ 37);
    let mut bridge = TimerBridge::default();

    pretty_assert_eq!(bridge.replace_handle(fresh_handle), None);
    let stale_lookup = bridge.resolve_fired(fired_host_timer(stale_handle));
    let active_timer_after_stale_callback = bridge.has_timer_id(TimerId::Recovery);
    let mismatched_lookup = bridge.resolve_fired(FiredHostTimer::new(
        fresh_handle.host_callback_id,
        mismatched_host_timer_id,
    ));
    let active_timer_after_mismatched_callback = bridge.has_timer_id(TimerId::Recovery);
    let fresh_lookup = bridge.resolve_fired(fired_host_timer(fresh_handle));
    let active_timer_after_fresh_callback = bridge.has_timer_id(TimerId::Recovery);

    let stale_transition = reduce(
        &state,
        timer_fired_event(stale_token, Millis::new(/*value*/ 151)),
    );
    let fresh_transition = reduce(
        &stale_transition.next,
        timer_fired_event(fresh_token, Millis::new(/*value*/ 152)),
    );

    pretty_assert_eq!(
        HostCallbackNoiseObservation {
            stale_lookup,
            active_timer_after_stale_callback,
            mismatched_lookup,
            active_timer_after_mismatched_callback,
            fresh_lookup,
            stale_reducer_effects: stale_transition.effects,
            active_timer_after_stale_reducer_event: stale_transition
                .next
                .timers()
                .active_token(TimerId::Recovery),
            active_timer_after_fresh_callback,
            active_timer_after_fresh_reducer_event: fresh_transition
                .next
                .timers()
                .active_token(TimerId::Recovery),
        },
        HostCallbackNoiseObservation {
            stale_lookup: FiredCoreTimerLookup::MissingHandle,
            active_timer_after_stale_callback: true,
            mismatched_lookup: FiredCoreTimerLookup::MismatchedHostTimerId {
                timer_id: TimerId::Recovery,
                expected: fresh_handle.host_timer_id,
            },
            active_timer_after_mismatched_callback: true,
            fresh_lookup: FiredCoreTimerLookup::Matched(fresh_handle),
            stale_reducer_effects: vec![Effect::RecordEventLoopMetric(
                EventLoopMetricEffect::StaleToken
            )],
            active_timer_after_stale_reducer_event: Some(fresh_token),
            active_timer_after_fresh_callback: false,
            active_timer_after_fresh_reducer_event: None,
        }
    );
}

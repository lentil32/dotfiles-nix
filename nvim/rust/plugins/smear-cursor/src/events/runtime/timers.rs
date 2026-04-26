use super::super::host_bridge::InstalledHostBridge;
use super::super::logging::trace_lazy;
use super::super::logging::warn;
use super::super::timer_protocol::FiredHostTimer;
use super::super::timer_protocol::HostCallbackId;
use super::super::timers::schedule_guarded;
use super::super::timers::start_timer_once;
use super::super::trace::timer_token_summary;
use super::RuntimeAccessResult;
use super::cell::restore_timer_bridge;
use super::cell::take_timer_bridge;
use super::telemetry::record_host_timer_rearm;
use super::telemetry::record_stale_token_event;
use super::telemetry::record_timer_fire_duration;
use super::telemetry::record_timer_schedule_duration;
use super::timer_bridge::CoreTimerHandle;
use super::timer_bridge::FiredCoreTimerLookup;
use super::timer_bridge::TimerBridge;
use super::timer_bridge::TimerBridgeRecoveryState;
use super::timer_bridge::TimerRetryTransition;
use super::with_core_read;
use crate::core::event::Event;
use crate::core::event::TimerFiredWithTokenEvent;
use crate::core::event::TimerLostWithTokenEvent;
use crate::core::runtime_reducer::as_delay_ms;
use crate::core::types::DelayBudgetMs;
use crate::core::types::Millis;
use crate::core::types::TimerId;
use crate::core::types::TimerToken;
use crate::host::HostBridgePort;
#[cfg(not(test))]
use crate::host::NeovimHost;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::panic::resume_unwind;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

fn with_timer_bridge<R>(accessor: impl FnOnce(&mut TimerBridge) -> R) -> RuntimeAccessResult<R> {
    let mut bridge = take_timer_bridge()?;
    let output = catch_unwind(AssertUnwindSafe(|| accessor(&mut bridge)));
    restore_timer_bridge(bridge);
    match output {
        Ok(output) => Ok(output),
        Err(panic_payload) => resume_unwind(panic_payload),
    }
}

#[cfg(test)]
pub(super) fn mutate_timer_bridge_for_test<R>(
    mutator: impl FnOnce(&mut TimerBridge) -> R,
) -> RuntimeAccessResult<R> {
    with_timer_bridge(mutator)
}

fn allocate_host_callback_id() -> RuntimeAccessResult<HostCallbackId> {
    with_timer_bridge(TimerBridge::allocate_host_callback_id)
}

#[cfg(not(test))]
fn set_core_timer_handle(handle: CoreTimerHandle) -> RuntimeAccessResult<bool> {
    set_core_timer_handle_with(&NeovimHost, handle)
}

#[cfg(test)]
fn set_core_timer_handle(handle: CoreTimerHandle) -> RuntimeAccessResult<bool> {
    set_core_timer_handle_with(&crate::host::FakeHostBridgePort::default(), handle)
}

fn set_core_timer_handle_with(
    host: &impl HostBridgePort,
    handle: CoreTimerHandle,
) -> RuntimeAccessResult<bool> {
    let displaced = with_timer_bridge(|bridge| bridge.replace_handle(handle))?;
    if let Some(displaced) = displaced.filter(|displaced| *displaced != handle) {
        stop_core_timer_handle_with(host, displaced, "replace");
    }
    Ok(displaced.is_some())
}

fn stop_core_timer_handle_with(
    host: &impl HostBridgePort,
    handle: CoreTimerHandle,
    context: &'static str,
) {
    let timer_id = handle.token.id();
    trace_lazy(|| {
        format!(
            "timer_stop context={} kind={} token={} callback_id={} host_timer_id={}",
            context,
            timer_id.name(),
            timer_token_summary(handle.token),
            handle.host_callback_id.get(),
            handle.host_timer_id.get(),
        )
    });
    if let Err(err) = InstalledHostBridge.stop_timer_with(host, handle.host_timer_id.get()) {
        warn(&format!(
            "failed to stop core timer (context={context}, kind={}, token={:?}, callback_id={}): {err}",
            timer_id.name(),
            handle.token,
            handle.host_callback_id.get(),
        ));
    }
}

fn stop_core_timer_handles_with(
    host: &impl HostBridgePort,
    handles: Vec<CoreTimerHandle>,
    context: &'static str,
) {
    for handle in handles {
        stop_core_timer_handle_with(host, handle, context);
    }
}

#[cfg(not(test))]
pub(super) fn stop_recovered_core_timer_handles(handles: Vec<CoreTimerHandle>) {
    stop_recovered_core_timer_handles_with(&NeovimHost, handles);
}

#[cfg(test)]
pub(super) fn stop_recovered_core_timer_handles(handles: Vec<CoreTimerHandle>) {
    stop_recovered_core_timer_handles_with(&crate::host::FakeHostBridgePort::default(), handles);
}

fn stop_recovered_core_timer_handles_with(
    host: &impl HostBridgePort,
    handles: Vec<CoreTimerHandle>,
) {
    stop_core_timer_handles_with(host, handles, "panic_recovery");
}

#[cfg(not(test))]
pub(crate) fn reset_core_timer_bridge() {
    reset_core_timer_bridge_with(&NeovimHost);
}

#[cfg(test)]
pub(crate) fn reset_core_timer_bridge() {
    reset_core_timer_bridge_with(&crate::host::FakeHostBridgePort::default());
}

fn reset_core_timer_bridge_with(host: &impl HostBridgePort) {
    match with_timer_bridge(TimerBridge::reset_transient) {
        Ok(drained) => stop_core_timer_handles_with(host, drained, "reset"),
        Err(err) => {
            warn(&format!(
                "timer bridge re-entered while resetting core timer bridge: {err}"
            ));
        }
    }
}

pub(super) fn capture_runtime_timer_bridge_recovery_state() -> TimerBridgeRecoveryState {
    match with_timer_bridge(|bridge| bridge.recovery_state()) {
        Ok(recovery_state) => recovery_state,
        Err(err) => {
            warn(&format!(
                "timer bridge re-entered while capturing core timer recovery state: {err}"
            ));
            TimerBridgeRecoveryState::default()
        }
    }
}

pub(super) fn clear_recovered_runtime_timer_bridge() {
    if let Err(err) = with_timer_bridge(TimerBridge::clear_recovered_transient) {
        warn(&format!(
            "timer bridge re-entered while clearing recovered core timers: {err}"
        ));
    }
}

fn resolve_fired_core_timer(
    fired_timer: FiredHostTimer,
) -> RuntimeAccessResult<FiredCoreTimerLookup> {
    with_timer_bridge(|bridge| bridge.resolve_fired(fired_timer))
}

fn stage_core_timer_dispatch_retry(retry: FiredHostTimer) {
    match with_timer_bridge(|bridge| bridge.stage_retry(retry)) {
        Ok(TimerRetryTransition::Staged) => {}
        Ok(TimerRetryTransition::Coalesced) => {
            trace_lazy(|| {
                format!(
                    "timer_fire_retry_coalesced callback_id={} host_timer_id={}",
                    retry.host_callback_id().get(),
                    retry.host_timer_id().get(),
                )
            });
            return;
        }
        Err(err) => {
            warn(&format!(
                "timer bridge re-entered while staging timer retry; scheduling uncoalesced retry: {err}"
            ));
        }
    }

    schedule_guarded("core timer dispatch retry", move || {
        // Release the single-flight slot before dispatch so a persistent reentry can enqueue
        // exactly one successor retry instead of wedging recovery behind a stale token.
        if let Err(err) = with_timer_bridge(|bridge| bridge.release_retry(retry)) {
            warn(&format!(
                "timer bridge re-entered while releasing timer retry: {err}"
            ));
        }
        dispatch_core_timer_fired(retry);
    });
}

pub(crate) fn dispatch_core_timer_fired(fired_timer: FiredHostTimer) {
    let started_at = Instant::now();
    let handle = match resolve_fired_core_timer(fired_timer) {
        Ok(FiredCoreTimerLookup::Matched(handle)) => handle,
        Ok(FiredCoreTimerLookup::MismatchedHostTimerId { timer_id, expected }) => {
            record_stale_token_event();
            warn(&format!(
                "core timer callback witness mismatch for kind={} callback_id={} expected_host_timer_id={} observed_host_timer_id={}",
                timer_id.name(),
                fired_timer.host_callback_id().get(),
                expected.get(),
                fired_timer.host_timer_id().get(),
            ));
            trace_lazy(|| {
                format!(
                    "timer_fire_ignored kind={} callback_id={} host_timer_id={} reason=mismatched_host_timer_id",
                    timer_id.name(),
                    fired_timer.host_callback_id().get(),
                    fired_timer.host_timer_id().get(),
                )
            });
            return;
        }
        Ok(FiredCoreTimerLookup::MissingHandle) => {
            trace_lazy(|| {
                format!(
                    "timer_fire_ignored callback_id={} host_timer_id={} reason=missing_handle",
                    fired_timer.host_callback_id().get(),
                    fired_timer.host_timer_id().get(),
                )
            });
            return;
        }
        Err(err) => {
            warn(&format!(
                "timer bridge re-entered while resolving timer fire; re-staging callback: {err}"
            ));
            stage_core_timer_dispatch_retry(fired_timer);
            return;
        }
    };

    let observed_at = to_core_millis(now_ms());
    trace_lazy(|| {
        format!(
            "timer_fire kind={} token={} callback_id={} host_timer_id={} observed_at={}",
            handle.token.id().name(),
            timer_token_summary(handle.token),
            handle.host_callback_id.get(),
            handle.host_timer_id.get(),
            observed_at.value(),
        )
    });

    let event = TimerFiredWithTokenEvent {
        token: handle.token,
        observed_at,
    };

    if let Err(err) = super::super::handlers::dispatch_core_event_with_default_scheduler(
        Event::TimerFiredWithToken(event),
    ) {
        warn(&format!(
            "runtime lane re-entered while dispatching timer event; re-staging for recovery: {err}"
        ));
        super::super::handlers::stage_core_event_with_default_scheduler(
            Event::TimerFiredWithToken(event),
        );
    }
    record_timer_fire_duration(duration_to_micros(started_at.elapsed()));
}

pub(crate) fn schedule_core_timer_effect(
    host_bridge: InstalledHostBridge,
    token: TimerToken,
    delay_ms: u64,
    requested_at: Millis,
) -> Vec<Event> {
    let timer_id = token.id();
    let host_callback_id = match allocate_host_callback_id() {
        Ok(host_callback_id) => host_callback_id,
        Err(err) => {
            warn(&format!(
                "timer bridge re-entered while allocating core timer callback id; dropping timer effect: {err}"
            ));
            return vec![Event::TimerLostWithToken(TimerLostWithTokenEvent {
                token,
                observed_at: requested_at,
            })];
        }
    };
    let timeout = Duration::from_millis(delay_ms);
    let timer_schedule_summary = format!(
        "kind={} token={} callback_id={} delay_ms={} requested_at={}",
        timer_id.name(),
        timer_token_summary(token),
        host_callback_id.get(),
        delay_ms,
        requested_at.value(),
    );
    let schedule_started_at = Instant::now();
    let schedule_outcome = start_timer_once(host_bridge, host_callback_id, timeout);
    record_timer_schedule_duration(duration_to_micros(schedule_started_at.elapsed()));
    match schedule_outcome {
        Ok(host_timer_id) => {
            trace_lazy(|| {
                format!(
                    "timer_schedule {} host_timer_id={}",
                    timer_schedule_summary,
                    host_timer_id.get(),
                )
            });
            match set_core_timer_handle(CoreTimerHandle {
                host_callback_id,
                host_timer_id,
                token,
            }) {
                Ok(rearmed) => {
                    if rearmed {
                        record_host_timer_rearm(timer_id);
                    }
                    Vec::new()
                }
                Err(err) => {
                    warn(&format!(
                        "timer bridge re-entered while recording core timer handle; stopping scheduled timer: {err}"
                    ));
                    #[cfg(not(test))]
                    let stop_result =
                        InstalledHostBridge.stop_timer_with(&NeovimHost, host_timer_id.get());
                    #[cfg(test)]
                    let stop_result = InstalledHostBridge.stop_timer_with(
                        &crate::host::FakeHostBridgePort::default(),
                        host_timer_id.get(),
                    );
                    if let Err(stop_err) = stop_result {
                        warn(&format!(
                            "failed to stop unslotted core timer after state re-entry: {stop_err}"
                        ));
                    }
                    vec![Event::TimerLostWithToken(TimerLostWithTokenEvent {
                        token,
                        observed_at: requested_at,
                    })]
                }
            }
        }
        Err(err) => {
            trace_lazy(|| format!("timer_schedule_failed {timer_schedule_summary} error={err}"));
            warn(&format!("failed to schedule core timer: {err}"));
            vec![Event::TimerLostWithToken(TimerLostWithTokenEvent {
                token,
                observed_at: requested_at,
            })]
        }
    }
}

pub(crate) fn resolved_timer_delay_ms(timer_id: TimerId, delay: DelayBudgetMs) -> u64 {
    if timer_id == TimerId::Animation && delay == DelayBudgetMs::DEFAULT_ANIMATION {
        return match with_core_read(|state| {
            let configured_interval_ms = state.runtime().config.time_interval;
            as_delay_ms(configured_interval_ms).max(1)
        }) {
            Ok(delay_ms) => delay_ms,
            Err(err) => {
                warn(&format!(
                    "runtime lane re-entered while resolving animation delay; using default timer budget: {err}"
                ));
                delay.value()
            }
        };
    }
    delay.value()
}

pub(crate) fn to_core_millis(value_ms: f64) -> Millis {
    if !value_ms.is_finite() || value_ms <= 0.0 {
        return Millis::new(0);
    }
    let Ok(duration) = Duration::try_from_secs_f64(value_ms / 1000.0) else {
        return Millis::new(u64::MAX);
    };
    Millis::new(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
}

pub(crate) fn duration_to_micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

pub(crate) fn now_ms() -> f64 {
    let duration = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration,
        Err(err) => err.duration(),
    };
    duration.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::TimerGeneration;
    use crate::events::timer_protocol::HostTimerId;
    use crate::host::FakeHostBridgePort;
    use crate::host::HostBridgeCall;
    use pretty_assertions::assert_eq;

    fn host_callback_id(value: i64) -> HostCallbackId {
        HostCallbackId::try_new(value).expect("test host callback id must be positive")
    }

    fn host_timer_id(value: i64) -> HostTimerId {
        HostTimerId::try_new(value).expect("test host timer id must be positive")
    }

    fn fired_timer(host_callback_id_value: i64, host_timer_id_value: i64) -> FiredHostTimer {
        FiredHostTimer::new(
            host_callback_id(host_callback_id_value),
            host_timer_id(host_timer_id_value),
        )
    }

    fn core_timer_handle(
        host_callback_id_value: i64,
        host_timer_id_value: i64,
        timer_id: TimerId,
        generation: u64,
    ) -> CoreTimerHandle {
        CoreTimerHandle {
            host_callback_id: host_callback_id(host_callback_id_value),
            host_timer_id: host_timer_id(host_timer_id_value),
            token: TimerToken::new(timer_id, TimerGeneration::new(generation)),
        }
    }

    #[test]
    fn timer_bridge_retry_release_reopens_the_retry_slot() {
        let retry = fired_timer(5, 11);
        let mut bridge = TimerBridge::default();

        assert_eq!(bridge.stage_retry(retry), TimerRetryTransition::Staged);
        bridge.release_retry(retry);
        assert_eq!(bridge.pending_retry_contains(retry), false);
        assert_eq!(bridge.stage_retry(retry), TimerRetryTransition::Staged);
    }

    #[test]
    fn timer_bridge_retry_keeps_distinct_callback_witnesses() {
        let first = fired_timer(7, 11);
        let second = fired_timer(8, 17);
        let mut bridge = TimerBridge::default();

        assert_eq!(bridge.stage_retry(first), TimerRetryTransition::Staged);
        assert_eq!(bridge.stage_retry(second), TimerRetryTransition::Staged);
        assert_eq!(bridge.pending_retry_len(), 2);
    }

    #[test]
    fn replacing_core_timer_handle_stops_displaced_host_timer_through_host_port() {
        mutate_timer_bridge_for_test(TimerBridge::reset_transient)
            .expect("timer bridge should be available before host-port replacement test");
        let host = FakeHostBridgePort::default();
        let first = core_timer_handle(
            /*host_callback_id_value*/ 41,
            /*host_timer_id_value*/ 43,
            TimerId::Animation,
            /*generation*/ 1,
        );
        let replacement = core_timer_handle(
            /*host_callback_id_value*/ 47,
            /*host_timer_id_value*/ 53,
            TimerId::Animation,
            /*generation*/ 2,
        );

        let first_replaced = set_core_timer_handle_with(&host, first)
            .expect("initial timer handle insert should succeed");
        let replacement_replaced = set_core_timer_handle_with(&host, replacement)
            .expect("replacement timer handle insert should succeed");
        let drained = mutate_timer_bridge_for_test(TimerBridge::reset_transient)
            .expect("timer bridge should be clearable after host-port replacement test");

        assert_eq!(
            (first_replaced, replacement_replaced, drained, host.calls()),
            (
                false,
                true,
                vec![replacement],
                vec![HostBridgeCall::StopTimer { timer_id: 43 }]
            )
        );
    }

    #[test]
    fn reset_core_timer_bridge_stops_drained_host_timers_through_host_port() {
        mutate_timer_bridge_for_test(TimerBridge::reset_transient)
            .expect("timer bridge should be available before host-port reset test");
        let host = FakeHostBridgePort::default();
        let animation = core_timer_handle(
            /*host_callback_id_value*/ 59,
            /*host_timer_id_value*/ 61,
            TimerId::Animation,
            /*generation*/ 3,
        );
        let recovery = core_timer_handle(
            /*host_callback_id_value*/ 67,
            /*host_timer_id_value*/ 71,
            TimerId::Recovery,
            /*generation*/ 5,
        );
        mutate_timer_bridge_for_test(|bridge| {
            assert_eq!(bridge.replace_handle(animation), None);
            assert_eq!(bridge.replace_handle(recovery), None);
        })
        .expect("timer bridge should accept primed handles");

        reset_core_timer_bridge_with(&host);

        let live_slots = mutate_timer_bridge_for_test(|bridge| {
            TimerId::ALL
                .into_iter()
                .map(|timer_id| (timer_id, bridge.has_timer_id(timer_id)))
                .collect::<Vec<_>>()
        })
        .expect("timer bridge should be readable after reset");
        assert_eq!(
            (live_slots, host.calls()),
            (
                vec![
                    (TimerId::Animation, false),
                    (TimerId::Ingress, false),
                    (TimerId::Recovery, false),
                    (TimerId::Cleanup, false),
                ],
                vec![
                    HostBridgeCall::StopTimer { timer_id: 61 },
                    HostBridgeCall::StopTimer { timer_id: 71 },
                ],
            )
        );
    }

    #[test]
    fn recovered_core_timer_handles_stop_through_host_port() {
        let host = FakeHostBridgePort::default();
        let animation = core_timer_handle(
            /*host_callback_id_value*/ 79,
            /*host_timer_id_value*/ 83,
            TimerId::Animation,
            /*generation*/ 8,
        );
        let cleanup = core_timer_handle(
            /*host_callback_id_value*/ 89,
            /*host_timer_id_value*/ 97,
            TimerId::Cleanup,
            /*generation*/ 13,
        );

        stop_recovered_core_timer_handles_with(&host, vec![animation, cleanup]);

        assert_eq!(
            host.calls(),
            vec![
                HostBridgeCall::StopTimer { timer_id: 83 },
                HostBridgeCall::StopTimer { timer_id: 97 },
            ]
        );
    }

    #[test]
    fn transient_reset_clears_timer_bridge_pending_dispatch_retries() {
        let animation = fired_timer(13, 5);
        let ingress = fired_timer(17, 8);

        mutate_timer_bridge_for_test(TimerBridge::clear_pending_retries)
            .expect("timer bridge should be available");
        mutate_timer_bridge_for_test(|bridge| {
            assert_eq!(bridge.stage_retry(animation), TimerRetryTransition::Staged);
            assert_eq!(bridge.stage_retry(ingress), TimerRetryTransition::Staged);
        })
        .expect("timer bridge should be available");

        super::super::diagnostics::reset_transient_event_state();

        assert_eq!(
            mutate_timer_bridge_for_test(|bridge| {
                (
                    bridge.pending_retry_contains(animation),
                    bridge.pending_retry_contains(ingress),
                )
            })
            .expect("timer bridge should be available"),
            (false, false)
        );
    }

    #[test]
    fn timer_bridge_reset_clears_handles_retries_and_callback_allocator() {
        let mut bridge = TimerBridge::default();
        let retry = fired_timer(23, 29);
        let handle = CoreTimerHandle {
            host_callback_id: host_callback_id(31),
            host_timer_id: host_timer_id(37),
            token: TimerToken::new(
                TimerId::Animation,
                crate::core::types::TimerGeneration::new(5),
            ),
        };

        assert_eq!(bridge.allocate_host_callback_id().get(), 1);
        assert_eq!(bridge.replace_handle(handle), None);
        assert_eq!(bridge.stage_retry(retry), TimerRetryTransition::Staged);

        assert_eq!(bridge.reset_transient(), vec![handle]);

        assert_eq!(bridge.has_timer_id(TimerId::Animation), false);
        assert_eq!(bridge.pending_retry_contains(retry), false);
        assert_eq!(bridge.allocate_host_callback_id().get(), 1);
    }
}

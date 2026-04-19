use super::super::host_bridge::InstalledHostBridge;
use super::super::logging::trace_lazy;
use super::super::logging::warn;
use super::super::timer_protocol::FiredHostTimer;
use super::super::timer_protocol::HostCallbackId;
use super::super::timer_protocol::HostTimerId;
use super::super::timers::schedule_guarded;
use super::super::timers::start_timer_once;
#[cfg(not(test))]
use super::super::timers::stop_timer;
use super::super::trace::timer_token_summary;
use super::EngineAccessResult;
use super::engine::mutate_engine_state;
use super::engine::read_engine_state;
use super::telemetry::record_host_timer_rearm;
use super::telemetry::record_stale_token_event;
use super::telemetry::record_timer_fire_duration;
use super::telemetry::record_timer_schedule_duration;
use crate::core::event::Event;
use crate::core::event::TimerFiredWithTokenEvent;
use crate::core::event::TimerLostWithTokenEvent;
use crate::core::runtime_reducer::as_delay_ms;
use crate::core::types::DelayBudgetMs;
use crate::core::types::Millis;
use crate::core::types::TimerId;
use crate::core::types::TimerSlots;
use crate::core::types::TimerToken;
use std::cell::RefCell;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct CoreTimerHandle {
    pub(crate) host_callback_id: HostCallbackId,
    pub(crate) host_timer_id: HostTimerId,
    pub(crate) token: TimerToken,
}

#[derive(Debug, Default)]
pub(crate) struct CoreTimerHandles {
    // One active host callback per reducer timer kind. The reducer token stays
    // authoritative; the callback id resolves the fired callback and the host
    // timer id remains the cancellation witness.
    slots: TimerSlots<Option<CoreTimerHandle>>,
}

impl CoreTimerHandles {
    fn slot(&self, timer_id: TimerId) -> Option<CoreTimerHandle> {
        self.slots.copied(timer_id)
    }

    pub(crate) fn replace(&mut self, handle: CoreTimerHandle) -> Option<CoreTimerHandle> {
        self.slots.replace(handle.token.id(), Some(handle))
    }

    #[cfg(test)]
    pub(crate) fn has_timer_id(&self, timer_id: TimerId) -> bool {
        self.slot(timer_id).is_some()
    }

    pub(crate) fn clear_all(&mut self) -> Vec<CoreTimerHandle> {
        self.slots.take_all()
    }

    #[cfg(test)]
    pub(crate) fn take_by_host_timer_id(
        &mut self,
        host_timer_id: HostTimerId,
    ) -> Option<CoreTimerHandle> {
        for slot in self.slots.iter_mut() {
            if slot
                .as_ref()
                .is_some_and(|handle| handle.host_timer_id == host_timer_id)
            {
                return slot.take();
            }
        }

        None
    }

    fn resolve_fired(&mut self, fired_timer: FiredHostTimer) -> FiredCoreTimerLookup {
        for timer_id in TimerId::ALL {
            let Some(handle) = self.slot(timer_id) else {
                continue;
            };
            if handle.host_callback_id != fired_timer.host_callback_id() {
                continue;
            }

            if handle.host_timer_id != fired_timer.host_timer_id() {
                return FiredCoreTimerLookup::MismatchedHostTimerId {
                    timer_id,
                    expected: handle.host_timer_id,
                };
            }

            let Some(handle) = self.slots.take(timer_id) else {
                warn("matched host timer slot unexpectedly lost its timer handle");
                return FiredCoreTimerLookup::MissingHandle;
            };
            return FiredCoreTimerLookup::Matched(handle);
        }

        FiredCoreTimerLookup::MissingHandle
    }

    #[cfg(test)]
    pub(crate) fn take_fired(&mut self, fired_timer: FiredHostTimer) -> Option<CoreTimerHandle> {
        match self.resolve_fired(fired_timer) {
            FiredCoreTimerLookup::Matched(handle) => Some(handle),
            FiredCoreTimerLookup::MismatchedHostTimerId { .. }
            | FiredCoreTimerLookup::MissingHandle => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum FiredCoreTimerLookup {
    Matched(CoreTimerHandle),
    MismatchedHostTimerId {
        timer_id: TimerId,
        expected: HostTimerId,
    },
    MissingHandle,
}

#[derive(Debug, Default)]
struct PendingCoreTimerRetries {
    retries: Vec<FiredHostTimer>,
}

impl PendingCoreTimerRetries {
    fn insert(&mut self, retry: FiredHostTimer) -> bool {
        if self.retries.contains(&retry) {
            return false;
        }

        self.retries.push(retry);
        true
    }

    fn remove(&mut self, retry: FiredHostTimer) {
        if let Some(index) = self.retries.iter().position(|pending| *pending == retry) {
            let _ = self.retries.swap_remove(index);
        }
    }

    fn clear(&mut self) {
        self.retries.clear();
    }

    #[cfg(test)]
    fn contains(&self, retry: FiredHostTimer) -> bool {
        self.retries.contains(&retry)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.retries.len()
    }
}

thread_local! {
    // Retry coalescing must live outside engine state because the reentry path cannot safely
    // borrow engine state just to decide whether another scheduled callback would be redundant.
    static PENDING_CORE_TIMER_RETRIES: RefCell<PendingCoreTimerRetries> =
        RefCell::new(PendingCoreTimerRetries::default());
}

fn with_pending_core_timer_retries<R>(
    mutator: impl FnOnce(&mut PendingCoreTimerRetries) -> R,
) -> R {
    PENDING_CORE_TIMER_RETRIES.with(|retries| {
        let mut retries = retries.borrow_mut();
        mutator(&mut retries)
    })
}

#[cfg(not(test))]
fn stop_host_timer(host_timer_id: HostTimerId) -> nvim_oxi::Result<()> {
    stop_timer(host_timer_id)
}

#[cfg(test)]
fn stop_host_timer(_host_timer_id: HostTimerId) -> nvim_oxi::Result<()> {
    Ok(())
}

fn allocate_host_callback_id() -> EngineAccessResult<HostCallbackId> {
    mutate_engine_state(|state| state.shell.allocate_host_callback_id())
}

fn set_core_timer_handle(handle: CoreTimerHandle) -> EngineAccessResult<bool> {
    let displaced = mutate_engine_state(|state| state.shell.core_timer_handles.replace(handle))?;
    if let Some(displaced) = displaced.filter(|displaced| *displaced != handle) {
        stop_core_timer_handle(displaced, "replace");
    }
    Ok(displaced.is_some())
}

fn stop_core_timer_handle(handle: CoreTimerHandle, context: &'static str) {
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
    if let Err(err) = stop_host_timer(handle.host_timer_id) {
        warn(&format!(
            "failed to stop core timer (context={context}, kind={}, token={:?}, callback_id={}): {err}",
            timer_id.name(),
            handle.token,
            handle.host_callback_id.get(),
        ));
    }
}

fn stop_core_timer_handles(handles: Vec<CoreTimerHandle>, context: &'static str) {
    for handle in handles {
        stop_core_timer_handle(handle, context);
    }
}

pub(super) fn stop_recovered_core_timer_handles(handles: Vec<CoreTimerHandle>) {
    stop_core_timer_handles(handles, "panic_recovery");
}

pub(crate) fn clear_all_core_timer_handles() {
    match mutate_engine_state(|state| state.shell.core_timer_handles.clear_all()) {
        Ok(drained) => stop_core_timer_handles(drained, "reset"),
        Err(err) => {
            warn(&format!(
                "engine state re-entered while draining core timers during reset: {err}"
            ));
        }
    }
}

pub(crate) fn clear_pending_core_timer_dispatch_retries() {
    with_pending_core_timer_retries(PendingCoreTimerRetries::clear);
}

fn resolve_fired_core_timer(
    fired_timer: FiredHostTimer,
) -> EngineAccessResult<FiredCoreTimerLookup> {
    mutate_engine_state(|state| state.shell.core_timer_handles.resolve_fired(fired_timer))
}

fn stage_core_timer_dispatch_retry(retry: FiredHostTimer) {
    if !with_pending_core_timer_retries(|retries| retries.insert(retry)) {
        trace_lazy(|| {
            format!(
                "timer_fire_retry_coalesced callback_id={} host_timer_id={}",
                retry.host_callback_id().get(),
                retry.host_timer_id().get(),
            )
        });
        return;
    }

    schedule_guarded("core timer dispatch retry", move || {
        // Release the single-flight slot before dispatch so a persistent reentry can enqueue
        // exactly one successor retry instead of wedging recovery behind a stale token.
        with_pending_core_timer_retries(|retries| retries.remove(retry));
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
                "engine state re-entered while resolving timer fire; re-staging callback: {err}"
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
            "engine state re-entered while dispatching timer event; re-staging for recovery: {err}"
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
                "engine state re-entered while allocating core timer callback id; dropping timer effect: {err}"
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
                        "engine state re-entered while recording core timer handle; stopping scheduled timer: {err}"
                    ));
                    if let Err(stop_err) = stop_host_timer(host_timer_id) {
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
        return match read_engine_state(|state| {
            let configured_interval_ms = state.core_state.runtime().config.time_interval;
            as_delay_ms(configured_interval_ms).max(1)
        }) {
            Ok(delay_ms) => delay_ms,
            Err(err) => {
                warn(&format!(
                    "engine state re-entered while resolving animation delay; using default timer budget: {err}"
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

    #[test]
    fn pending_core_timer_retry_coalesces_matching_callback_payloads() {
        let retry = fired_timer(3, 7);
        clear_pending_core_timer_dispatch_retries();

        assert!(with_pending_core_timer_retries(
            |retries| retries.insert(retry)
        ));
        assert!(!with_pending_core_timer_retries(
            |retries| retries.insert(retry)
        ));
        assert!(with_pending_core_timer_retries(
            |retries| retries.contains(retry)
        ));

        clear_pending_core_timer_dispatch_retries();
    }

    #[test]
    fn pending_core_timer_retry_remove_releases_the_retry_entry() {
        let retry = fired_timer(5, 11);
        clear_pending_core_timer_dispatch_retries();

        assert!(with_pending_core_timer_retries(
            |retries| retries.insert(retry)
        ));
        with_pending_core_timer_retries(|retries| retries.remove(retry));

        assert!(!with_pending_core_timer_retries(
            |retries| retries.contains(retry)
        ));
        assert!(with_pending_core_timer_retries(
            |retries| retries.insert(retry)
        ));

        clear_pending_core_timer_dispatch_retries();
    }

    #[test]
    fn pending_core_timer_retry_keeps_distinct_callback_witnesses() {
        let first = fired_timer(7, 11);
        let second = fired_timer(8, 17);
        clear_pending_core_timer_dispatch_retries();

        assert!(with_pending_core_timer_retries(
            |retries| retries.insert(first)
        ));
        assert!(with_pending_core_timer_retries(
            |retries| retries.insert(second)
        ));
        assert_eq!(with_pending_core_timer_retries(|retries| retries.len()), 2);

        clear_pending_core_timer_dispatch_retries();
    }

    #[test]
    fn clearing_pending_core_timer_retries_drops_all_host_timer_witnesses() {
        let animation = fired_timer(11, 3);
        let cleanup = fired_timer(19, 29);

        clear_pending_core_timer_dispatch_retries();
        assert!(with_pending_core_timer_retries(
            |retries| retries.insert(animation)
        ));
        assert!(with_pending_core_timer_retries(
            |retries| retries.insert(cleanup)
        ));

        clear_pending_core_timer_dispatch_retries();

        assert_eq!(
            with_pending_core_timer_retries(|retries| {
                (retries.contains(animation), retries.contains(cleanup))
            }),
            (false, false)
        );
    }

    #[test]
    fn transient_reset_clears_pending_core_timer_dispatch_retries() {
        let animation = fired_timer(13, 5);
        let ingress = fired_timer(17, 8);

        clear_pending_core_timer_dispatch_retries();
        assert!(with_pending_core_timer_retries(
            |retries| retries.insert(animation)
        ));
        assert!(with_pending_core_timer_retries(
            |retries| retries.insert(ingress)
        ));

        super::super::diagnostics::reset_transient_event_state();

        assert_eq!(
            with_pending_core_timer_retries(|retries| {
                (retries.contains(animation), retries.contains(ingress))
            }),
            (false, false)
        );
    }
}

use super::super::host_bridge::InstalledHostBridge;
use super::super::logging::trace_lazy;
use super::super::logging::warn;
use super::super::timers::NvimTimerId;
use super::super::timers::start_timer_once;
use super::super::timers::stop_timer;
use super::super::trace::timer_kind_name;
use super::super::trace::timer_token_summary;
use super::engine::read_engine_state;
use super::telemetry::record_host_timer_rearm;
use super::telemetry::record_timer_fire_duration;
use super::telemetry::record_timer_schedule_duration;
use crate::core::effect::TimerKind;
use crate::core::event::Event;
use crate::core::event::TimerFiredWithTokenEvent;
use crate::core::event::TimerLostWithTokenEvent;
use crate::core::runtime_reducer::as_delay_ms;
use crate::core::types::DelayBudgetMs;
use crate::core::types::Millis;
use crate::core::types::TimerToken;
use std::cell::RefCell;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct CoreTimerHandle {
    pub(crate) shell_timer_id: NvimTimerId,
    pub(crate) token: TimerToken,
}

#[derive(Default)]
pub(crate) struct CoreTimerHandles {
    handles: Vec<CoreTimerHandle>,
}

impl CoreTimerHandles {
    pub(crate) fn replace(&mut self, handle: CoreTimerHandle) -> Option<CoreTimerHandle> {
        let timer_id = handle.token.id();
        if let Some(index) = self
            .handles
            .iter()
            .position(|existing| existing.token.id() == timer_id)
        {
            let displaced = std::mem::replace(&mut self.handles[index], handle);
            Some(displaced)
        } else {
            self.handles.push(handle);
            None
        }
    }

    #[cfg(test)]
    pub(crate) fn has_outstanding_timer_id(&self, timer_id: crate::core::types::TimerId) -> bool {
        self.handles
            .iter()
            .any(|handle| handle.token.id() == timer_id)
    }

    pub(crate) fn clear_all(&mut self) -> Vec<CoreTimerHandle> {
        self.handles.drain(..).collect()
    }

    pub(crate) fn take_by_shell_timer_id(
        &mut self,
        shell_timer_id: NvimTimerId,
    ) -> Option<CoreTimerHandle> {
        self.handles
            .iter()
            .position(|handle| handle.shell_timer_id == shell_timer_id)
            .map(|index| self.handles.swap_remove(index))
    }
}

thread_local! {
    // Reducer token generations define which timer edge is live. The runtime keeps a single
    // outstanding shell timer per TimerId, replacing the prior host timer when a newer token is
    // scheduled for the same kind.
    static CORE_TIMER_HANDLES: RefCell<CoreTimerHandles> =
        RefCell::new(CoreTimerHandles::default());
}

fn with_core_timer_handles<R>(f: impl FnOnce(&mut CoreTimerHandles) -> R) -> R {
    CORE_TIMER_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        f(&mut handles)
    })
}

fn set_core_timer_handle(handle: CoreTimerHandle) -> bool {
    let displaced = with_core_timer_handles(|handles| handles.replace(handle));
    if let Some(displaced) = displaced {
        stop_core_timer_handle(displaced, "replace");
    }
    displaced.is_some()
}

fn stop_core_timer_handle(handle: CoreTimerHandle, context: &'static str) {
    let kind = TimerKind::from_timer_id(handle.token.id());
    trace_lazy(|| {
        format!(
            "timer_stop context={} kind={} token={} shell_timer_id={}",
            context,
            timer_kind_name(kind),
            timer_token_summary(handle.token),
            handle.shell_timer_id.get(),
        )
    });
    if let Err(err) = stop_timer(handle.shell_timer_id) {
        warn(&format!(
            "failed to stop core timer (context={context}, kind={:?}, token={:?}): {err}",
            kind, handle.token
        ));
    }
}

pub(crate) fn clear_all_core_timer_handles() {
    let drained = with_core_timer_handles(CoreTimerHandles::clear_all);
    for handle in drained {
        stop_core_timer_handle(handle, "reset");
    }
}

fn take_core_timer_handle_by_shell_timer_id(
    shell_timer_id: NvimTimerId,
) -> Option<CoreTimerHandle> {
    with_core_timer_handles(|handles| handles.take_by_shell_timer_id(shell_timer_id))
}

pub(crate) fn dispatch_shell_timer_fired(shell_timer_id: NvimTimerId) {
    let started_at = Instant::now();
    let Some(handle) = take_core_timer_handle_by_shell_timer_id(shell_timer_id) else {
        trace_lazy(|| {
            format!(
                "timer_fire_ignored shell_timer_id={} reason=missing_handle",
                shell_timer_id.get(),
            )
        });
        return;
    };

    let observed_at = to_core_millis(now_ms());
    trace_lazy(|| {
        format!(
            "timer_fire kind={} token={} shell_timer_id={} observed_at={}",
            timer_kind_name(TimerKind::from_timer_id(handle.token.id())),
            timer_token_summary(handle.token),
            shell_timer_id.get(),
            observed_at.value(),
        )
    });

    let event = Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
        token: handle.token,
        observed_at,
    });

    if let Err(err) =
        super::super::handlers::dispatch_core_event_with_default_scheduler(event.clone())
    {
        warn(&format!(
            "engine state re-entered while dispatching timer event; re-staging for recovery: {err}"
        ));
        super::super::handlers::stage_core_event_with_default_scheduler(event);
    }
    record_timer_fire_duration(duration_to_micros(started_at.elapsed()));
}

pub(crate) fn schedule_core_timer_effect(
    host_bridge: InstalledHostBridge,
    token: TimerToken,
    delay_ms: u64,
    requested_at: Millis,
) -> Vec<Event> {
    let kind = TimerKind::from_timer_id(token.id());
    let timeout = Duration::from_millis(delay_ms);
    let timer_schedule_summary = format!(
        "kind={} token={} delay_ms={} requested_at={}",
        timer_kind_name(kind),
        timer_token_summary(token),
        delay_ms,
        requested_at.value(),
    );
    let schedule_started_at = Instant::now();
    let schedule_outcome = start_timer_once(host_bridge, timeout);
    record_timer_schedule_duration(duration_to_micros(schedule_started_at.elapsed()));
    match schedule_outcome {
        Ok(shell_timer_id) => {
            trace_lazy(|| {
                format!(
                    "timer_schedule {} shell_timer_id={}",
                    timer_schedule_summary,
                    shell_timer_id.get(),
                )
            });
            let rearmed = set_core_timer_handle(CoreTimerHandle {
                shell_timer_id,
                token,
            });
            if rearmed {
                record_host_timer_rearm(kind);
            }
            Vec::new()
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

pub(crate) fn resolved_timer_delay_ms(kind: TimerKind, delay: DelayBudgetMs) -> u64 {
    if kind == TimerKind::Animation && delay == DelayBudgetMs::DEFAULT_ANIMATION {
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

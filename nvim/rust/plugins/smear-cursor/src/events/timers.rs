use super::host_bridge::InstalledHostBridge;
use super::logging::trace_lazy;
use super::logging::warn;
use super::runtime::record_effect_failure;
use super::timer_protocol::FiredHostTimer;
use super::timer_protocol::HostCallbackId;
use super::timer_protocol::HostTimerId;
use crate::core::event::EffectFailureSource;
use crate::host::NeovimHost;
use crate::host::SchedulerPort;
use nvim_oxi::Result;
use std::time::Duration;

// Core timers use Neovim's builtin timer API so Rust owns cancellation state
// while the host owns timer allocation and teardown.

pub(crate) fn schedule_guarded(context: &'static str, callback: impl FnOnce() + 'static) {
    schedule_guarded_with(&NeovimHost, context, callback);
}

fn schedule_guarded_with(
    host: &impl SchedulerPort,
    context: &'static str,
    callback: impl FnOnce() + 'static,
) {
    let mut callback = Some(callback);
    host.schedule(Box::new(move || {
        let Some(callback) = callback.take() else {
            // Surprising: scheduled callback was invoked after being consumed.
            warn("scheduled callback invoked after callback was consumed");
            return;
        };

        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback)).is_err() {
            warn(&format!("scheduled callback panicked: {context}"));
            record_effect_failure(EffectFailureSource::ScheduledCallback, context);
        }
    }));
}

pub(super) fn start_timer_once(
    host_bridge: InstalledHostBridge,
    host_callback_id: HostCallbackId,
    timeout: Duration,
) -> Result<HostTimerId> {
    let timeout_ms = i64::try_from(timeout.as_millis()).unwrap_or(i64::MAX);
    HostTimerId::try_new(host_bridge.start_timer_once(host_callback_id, timeout_ms)?)
}

pub(crate) fn on_core_timer_fired_event(host_callback_id: i64, host_timer_id: i64) {
    let fired_timer = FiredHostTimer::try_from_raw(host_callback_id, host_timer_id);
    schedule_guarded("core timer dispatch", move || {
        let fired_timer = match fired_timer {
            Ok(fired_timer) => fired_timer,
            Err(err) => {
                warn(&format!(
                    "core timer callback received invalid host timer payload: {err}"
                ));
                return;
            }
        };

        trace_lazy(|| {
            format!(
                "host_timer_callback callback_id={} host_timer_id={} result=queued",
                fired_timer.host_callback_id().get(),
                fired_timer.host_timer_id().get(),
            )
        });
        super::runtime::dispatch_core_timer_fired(fired_timer);
    });
}

#[cfg(test)]
mod tests {
    use super::schedule_guarded_with;
    use crate::host::FakeSchedulerPort;
    use crate::host::SchedulerCall;
    use pretty_assertions::assert_eq;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn schedule_guarded_routes_through_scheduler_port() {
        let host = FakeSchedulerPort::default();
        let callback_ran = Rc::new(Cell::new(false));
        let callback_witness = Rc::clone(&callback_ran);

        schedule_guarded_with(&host, "scheduler route test", move || {
            callback_witness.set(true);
        });

        assert_eq!(host.calls(), vec![SchedulerCall::Schedule]);
        assert!(callback_ran.get());
    }
}

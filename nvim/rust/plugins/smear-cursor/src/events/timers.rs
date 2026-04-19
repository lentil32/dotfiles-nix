use super::host_bridge::InstalledHostBridge;
use super::logging::trace_lazy;
use super::logging::warn;
use super::runtime::record_effect_failure;
use crate::core::event::EffectFailureSource;
use crate::core::types::TimerToken;
use nvim_oxi::Result;
use nvim_oxi::schedule;
use std::num::NonZeroI64;
use std::time::Duration;

// Surprising: the local nvim-oxi libuv timer wrapper never closes its raw
// handle on drop, so core timers use a Lua-owned persistent timer bridge.

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(super) struct NvimTimerId(NonZeroI64);

impl NvimTimerId {
    pub(super) fn try_new(value: i64) -> Result<Self> {
        NonZeroI64::new(value)
            .filter(|timer_id| timer_id.get() > 0)
            .map_or_else(
                || {
                    Err(nvim_oxi::api::Error::Other(format!(
                        "timer_start returned invalid timer id: {value}"
                    ))
                    .into())
                },
                |timer_id| Ok(Self(timer_id)),
            )
    }

    pub(super) const fn get(self) -> i64 {
        self.0.get()
    }
}

pub(crate) fn schedule_guarded(context: &'static str, callback: impl FnOnce() + 'static) {
    let mut callback = Some(callback);
    schedule(move |()| {
        let Some(callback) = callback.take() else {
            // Surprising: scheduled callback was invoked after being consumed.
            warn("scheduled callback invoked after callback was consumed");
            return;
        };

        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback)).is_err() {
            warn(&format!("scheduled callback panicked: {context}"));
            record_effect_failure(EffectFailureSource::ScheduledCallback, context);
        }
    });
}

pub(super) fn start_timer_once(
    host_bridge: InstalledHostBridge,
    token: TimerToken,
    timeout: Duration,
) -> Result<NvimTimerId> {
    let timeout_ms = i64::try_from(timeout.as_millis()).unwrap_or(i64::MAX);
    let timer_id = host_bridge.start_timer_once(
        i64::from(timer_slot_id(token)),
        token.generation().value(),
        timeout_ms,
    )?;
    NvimTimerId::try_new(timer_id)
}

pub(super) fn stop_timer(timer_id: NvimTimerId) -> Result<()> {
    Ok(InstalledHostBridge.stop_timer(timer_id.get())?)
}

pub(crate) fn on_core_timer_event(timer_id: i64) {
    on_core_timer_slot_event(timer_id, 0);
}

pub(crate) fn on_core_timer_slot_event(timer_id: i64, generation: u64) {
    let timer_id = NvimTimerId::try_new(timer_id);
    schedule_guarded("core timer dispatch", move || {
        let timer_id = match timer_id {
            Ok(timer_id) => timer_id,
            Err(err) => {
                warn(&format!(
                    "core timer callback received invalid timer id: {err}"
                ));
                return;
            }
        };

        trace_lazy(|| {
            format!(
                "shell_timer_callback shell_timer_id={} generation={} result=queued",
                timer_id.get(),
                generation,
            )
        });
        super::runtime::dispatch_shell_timer_fired(timer_id, generation);
    });
}

fn timer_slot_id(token: TimerToken) -> u8 {
    match token.id() {
        crate::core::types::TimerId::Animation => 1,
        crate::core::types::TimerId::Ingress => 2,
        crate::core::types::TimerId::Recovery => 3,
        crate::core::types::TimerId::Cleanup => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timer_id(value: i64) -> NvimTimerId {
        NvimTimerId::try_new(value).expect("test timer id must be positive")
    }

    #[test]
    fn nvim_timer_id_rejects_non_positive_values() {
        assert!(NvimTimerId::try_new(0).is_err());
        assert!(NvimTimerId::try_new(-7).is_err());
        assert_eq!(timer_id(13).get(), 13);
    }
}

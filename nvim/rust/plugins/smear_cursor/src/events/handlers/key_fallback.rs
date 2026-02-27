use super::super::policy::BufferEventPolicy;
use super::core_dispatch::dispatch_core_events_with_default_scheduler;
use crate::core::event::{Event as CoreEvent, InitializeEvent, KeyFallbackQueuedEvent};
use crate::core::types::Millis;
use nvim_oxi::Result;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum KeyEventAction {
    NoAction,
    QueueKeyFallback { delay_ms: u64 },
}

const MIN_DEBOUNCED_KEY_FALLBACK_DELAY_MS: u64 = 1;

pub(crate) fn decide_key_event_action(
    policy: BufferEventPolicy,
    key_delay_ms: u64,
) -> KeyEventAction {
    if !policy.use_key_fallback() {
        return KeyEventAction::NoAction;
    }
    KeyEventAction::QueueKeyFallback {
        // on_key runs before motion side effects are visible. A zero-delay fallback
        // can probe a stale pre-move cursor and create phantom retargets.
        delay_ms: key_delay_ms.max(MIN_DEBOUNCED_KEY_FALLBACK_DELAY_MS),
    }
}

fn build_key_fallback_events(
    needs_initialize: bool,
    observed_at: Millis,
    due_at: Millis,
) -> Vec<CoreEvent> {
    let mut events = Vec::with_capacity(1 + usize::from(needs_initialize));
    if needs_initialize {
        events.push(CoreEvent::Initialize(InitializeEvent { observed_at }));
    }
    events.push(CoreEvent::KeyFallbackQueued(KeyFallbackQueuedEvent {
        observed_at,
        due_at,
    }));
    events
}

pub(super) fn dispatch_core_key_fallback_queued(
    needs_initialize: bool,
    observed_at: Millis,
    due_at: Millis,
) -> Result<()> {
    dispatch_core_events_with_default_scheduler(build_key_fallback_events(
        needs_initialize,
        observed_at,
        due_at,
    ))
}

#[cfg(test)]
mod tests {
    use super::{build_key_fallback_events, decide_key_event_action};
    use crate::core::event::{Event as CoreEvent, InitializeEvent, KeyFallbackQueuedEvent};
    use crate::core::types::Millis;
    use crate::events::policy::BufferEventPolicy;

    #[test]
    fn key_fallback_builder_prepends_initialize_when_needed() {
        let observed_at = Millis::new(11);
        let due_at = Millis::new(17);

        let events = build_key_fallback_events(true, observed_at, due_at);

        assert_eq!(
            events,
            vec![
                CoreEvent::Initialize(InitializeEvent { observed_at }),
                CoreEvent::KeyFallbackQueued(KeyFallbackQueuedEvent {
                    observed_at,
                    due_at,
                }),
            ]
        );
    }

    #[test]
    fn key_fallback_builder_skips_initialize_when_already_primed() {
        let observed_at = Millis::new(11);
        let due_at = Millis::new(17);

        let events = build_key_fallback_events(false, observed_at, due_at);

        assert_eq!(
            events,
            vec![CoreEvent::KeyFallbackQueued(KeyFallbackQueuedEvent {
                observed_at,
                due_at,
            })]
        );
    }

    #[test]
    fn key_fallback_decision_debounces_zero_delay() {
        let action = decide_key_event_action(BufferEventPolicy::Normal, 0);
        assert_eq!(
            action,
            super::KeyEventAction::QueueKeyFallback { delay_ms: 1 }
        );
    }
}

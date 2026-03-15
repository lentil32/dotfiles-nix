use super::super::cursor::{mode_string, smear_outside_cmd_row};
use super::super::ingress::{AutocmdIngress, Ingress, parse_autocmd_ingress};
use super::super::logging::warn;
use super::super::policy::{current_buffer_event_policy, skip_current_buffer_events};
use super::super::runtime::{
    IngressReadSnapshot, ingress_read_snapshot, is_enabled, note_autocmd_event_now,
    note_cursor_color_colorscheme_change, now_ms, record_effect_failure, record_ingress_applied,
    record_ingress_dropped, record_ingress_received, to_core_millis,
};
use super::core_dispatch::{
    dispatch_core_event_with_default_scheduler, dispatch_core_events_with_default_scheduler,
};
use super::key_fallback::{
    KeyEventAction, decide_key_event_action, dispatch_core_key_fallback_queued,
};
use super::source_selection::should_request_observation_for_autocmd;
use crate::core::effect::IngressCursorPresentationRequest;
use crate::core::event::{
    EffectFailureSource, Event as CoreEvent, ExternalDemandQueuedEvent, InitializeEvent,
};
use crate::core::state::ExternalDemandKind;
use crate::core::types::Millis;
use crate::draw::clear_highlight_cache;
use crate::types::ScreenCell;
use nvim_oxi::{Result, api};
use thiserror::Error;

fn build_cursor_autocmd_events(
    ingress: AutocmdIngress,
    observed_at: Millis,
    needs_initialize: bool,
    ingress_cursor_presentation: IngressCursorPresentationRequest,
) -> Vec<CoreEvent> {
    let should_request_observation = should_request_observation_for_autocmd(ingress);
    let should_dispatch_mode_changed = ingress.requests_mode_changed_dispatch();
    let mut events = Vec::with_capacity(
        usize::from(needs_initialize)
            + usize::from(should_dispatch_mode_changed)
            + usize::from(should_request_observation),
    );

    if needs_initialize {
        events.push(CoreEvent::Initialize(InitializeEvent { observed_at }));
    }

    if should_dispatch_mode_changed {
        events.push(CoreEvent::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ModeChanged,
            observed_at,
            requested_target: None,
            ingress_cursor_presentation: Some(ingress_cursor_presentation),
        }));
    }

    if should_request_observation {
        events.push(CoreEvent::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at,
            requested_target: None,
            ingress_cursor_presentation: if should_dispatch_mode_changed {
                None
            } else {
                Some(ingress_cursor_presentation)
            },
        }));
    }

    events
}

fn on_key_ingress() -> Result<bool> {
    let snapshot = ingress_read_snapshot();
    if !snapshot.enabled() {
        return Ok(false);
    }

    let buffer = api::get_current_buf();
    if !buffer.is_valid() {
        return Ok(false);
    }
    let policy = current_buffer_event_policy(&buffer);
    if skip_current_buffer_events(&snapshot, &buffer)? {
        return Ok(false);
    }

    let action = decide_key_event_action(policy, snapshot.key_delay_ms());
    match action {
        KeyEventAction::NoAction => Ok(false),
        KeyEventAction::QueueKeyFallback { delay_ms } => {
            let observed_at = to_core_millis(now_ms());
            let due_at = Millis::new(observed_at.value().saturating_add(delay_ms));
            dispatch_core_key_fallback_queued(snapshot.needs_initialize(), observed_at, due_at);
            Ok(true)
        }
    }
}

fn collect_ingress_cursor_presentation_request(
    snapshot: &IngressReadSnapshot,
) -> Result<IngressCursorPresentationRequest> {
    let mode = mode_string();
    let current_corners = snapshot.current_corners();
    let mode_allowed = snapshot.mode_allowed(&mode);

    Ok(IngressCursorPresentationRequest::new(
        mode_allowed,
        smear_outside_cmd_row(&current_corners)?,
        ScreenCell::from_rounded_point(current_corners[0]),
    ))
}

fn on_cursor_event_core_for_autocmd(ingress: AutocmdIngress) -> bool {
    let snapshot = ingress_read_snapshot();
    if !snapshot.enabled() {
        return false;
    }

    let buffer = api::get_current_buf();
    if !buffer.is_valid() {
        return false;
    }
    match skip_current_buffer_events(&snapshot, &buffer) {
        Ok(true) => return false,
        Ok(false) => {}
        Err(err) => {
            warn(&format!("core cursor buffer policy failed: {err}"));
            return false;
        }
    }
    note_autocmd_event_now();

    let ingress_cursor_presentation = match collect_ingress_cursor_presentation_request(&snapshot) {
        Ok(request) => request,
        Err(err) => {
            warn(&format!(
                "core cursor presentation policy probe failed: {err}"
            ));
            return false;
        }
    };

    let observed_at = to_core_millis(now_ms());
    let events = build_cursor_autocmd_events(
        ingress,
        observed_at,
        snapshot.needs_initialize(),
        ingress_cursor_presentation,
    );

    if events.is_empty() {
        return false;
    }

    dispatch_core_events_with_default_scheduler(events);
    true
}

fn on_buf_enter_impl() -> bool {
    if !is_enabled() {
        return false;
    }

    let buffer = api::get_current_buf();
    if !buffer.is_valid() {
        return false;
    }

    dispatch_core_event_with_default_scheduler(CoreEvent::ExternalDemandQueued(
        ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::BufferEntered,
            observed_at: to_core_millis(now_ms()),
            requested_target: None,
            ingress_cursor_presentation: None,
        },
    ));

    true
}

fn on_colorscheme_impl() -> bool {
    clear_highlight_cache();
    note_cursor_color_colorscheme_change();
    true
}

fn on_autocmd_ingress(ingress: AutocmdIngress) -> bool {
    if ingress.is_buffer_enter() {
        return on_buf_enter_impl();
    }
    if ingress.is_colorscheme() {
        return on_colorscheme_impl();
    }
    if ingress == AutocmdIngress::Unknown {
        return false;
    }

    on_cursor_event_core_for_autocmd(ingress)
}

fn dispatch_ingress(ingress: Ingress) -> Result<()> {
    record_ingress_received();
    let outcome = match ingress {
        Ingress::KeyObserved => on_key_ingress(),
        Ingress::Autocmd(autocmd_ingress) => Ok(on_autocmd_ingress(autocmd_ingress)),
    };
    match outcome {
        Ok(applied) => {
            if applied {
                record_ingress_applied();
            } else {
                record_ingress_dropped();
            }
            Ok(())
        }
        Err(err) => {
            record_ingress_dropped();
            Err(err)
        }
    }
}

#[derive(Debug, Error)]
enum IngressInvocationFailure {
    #[error("dispatch failed: {0}")]
    Dispatch(#[source] nvim_oxi::Error),
}

enum InfallibleIngressInvocation {
    Completed,
    Failed(IngressInvocationFailure),
    Panicked,
}

fn invoke_infallible_ingress(callback: impl FnOnce() -> Result<()>) -> InfallibleIngressInvocation {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback)) {
        Ok(Ok(())) => InfallibleIngressInvocation::Completed,
        Ok(Err(err)) => {
            InfallibleIngressInvocation::Failed(IngressInvocationFailure::Dispatch(err))
        }
        Err(_) => InfallibleIngressInvocation::Panicked,
    }
}

pub(crate) fn on_key_listener_event() {
    match invoke_infallible_ingress(on_key_event) {
        InfallibleIngressInvocation::Completed => {}
        InfallibleIngressInvocation::Failed(error) => {
            warn(&format!("on_key ingress failed: {error}"));
            record_effect_failure(EffectFailureSource::KeyListener, "on_key");
        }
        InfallibleIngressInvocation::Panicked => {
            warn("on_key ingress panicked");
            record_effect_failure(EffectFailureSource::KeyListener, "on_key");
        }
    }
}

fn on_key_event() -> Result<()> {
    dispatch_ingress(Ingress::KeyObserved)
}

pub(crate) fn on_autocmd_event(event: &str) -> Result<()> {
    dispatch_ingress(Ingress::Autocmd(parse_autocmd_ingress(event)))
}

#[cfg(test)]
mod tests {
    use super::{build_cursor_autocmd_events, invoke_infallible_ingress};
    use crate::core::effect::IngressCursorPresentationRequest;
    use crate::core::event::{Event as CoreEvent, ExternalDemandQueuedEvent, InitializeEvent};
    use crate::core::state::ExternalDemandKind;
    use crate::core::types::Millis;
    use crate::events::ingress::AutocmdIngress;

    fn presentation() -> IngressCursorPresentationRequest {
        IngressCursorPresentationRequest::new(true, true, None)
    }

    #[test]
    fn cursor_autocmd_builder_batches_initialize_and_cursor_observation() {
        let observed_at = Millis::new(21);

        let events = build_cursor_autocmd_events(
            AutocmdIngress::CursorMoved,
            observed_at,
            true,
            presentation(),
        );

        assert_eq!(
            events,
            vec![
                CoreEvent::Initialize(InitializeEvent { observed_at }),
                CoreEvent::ExternalDemandQueued(ExternalDemandQueuedEvent {
                    kind: ExternalDemandKind::ExternalCursor,
                    observed_at,
                    requested_target: None,
                    ingress_cursor_presentation: Some(presentation()),
                }),
            ]
        );
    }

    #[test]
    fn cursor_autocmd_builder_preserves_single_presentation_owner_on_mode_change() {
        let observed_at = Millis::new(21);

        let events = build_cursor_autocmd_events(
            AutocmdIngress::ModeChanged,
            observed_at,
            false,
            presentation(),
        );

        assert_eq!(
            events,
            vec![CoreEvent::ExternalDemandQueued(ExternalDemandQueuedEvent {
                kind: ExternalDemandKind::ModeChanged,
                observed_at,
                requested_target: None,
                ingress_cursor_presentation: Some(presentation()),
            })]
        );
    }

    #[test]
    fn invoke_infallible_ingress_reports_success() {
        let outcome = invoke_infallible_ingress(|| Ok(()));

        assert!(matches!(
            outcome,
            super::InfallibleIngressInvocation::Completed
        ));
    }

    #[test]
    fn invoke_infallible_ingress_reports_failure_without_panicking() {
        let outcome = invoke_infallible_ingress(|| {
            Err(nvim_oxi::api::Error::Other("ingress failed".into()).into())
        });

        assert!(matches!(
            outcome,
            super::InfallibleIngressInvocation::Failed(
                super::IngressInvocationFailure::Dispatch(error)
            ) if error.to_string().contains("ingress failed")
        ));
    }

    #[test]
    fn invoke_infallible_ingress_reports_panic_without_propagating() {
        let outcome = invoke_infallible_ingress(|| -> nvim_oxi::Result<()> {
            panic!("listener panic");
        });

        assert!(matches!(
            outcome,
            super::InfallibleIngressInvocation::Panicked
        ));
    }
}

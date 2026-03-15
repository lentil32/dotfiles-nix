use super::super::cursor::{mode_string, smear_outside_cmd_row};
use super::super::ingress::{AutocmdIngress, Ingress, parse_autocmd_ingress};
use super::super::logging::warn;
use super::super::policy::skip_current_buffer_events;
use super::super::runtime::{
    IngressReadSnapshot, ingress_read_snapshot, note_autocmd_event_now,
    note_cursor_color_colorscheme_change, now_ms, record_ingress_applied, record_ingress_coalesced,
    record_ingress_dropped, record_ingress_received, to_core_millis,
};
use super::core_dispatch::dispatch_core_events_with_default_scheduler;
use super::source_selection::should_request_observation_for_autocmd;
use crate::core::effect::IngressCursorPresentationRequest;
use crate::core::event::{Event as CoreEvent, ExternalDemandQueuedEvent, InitializeEvent};
use crate::core::state::ExternalDemandKind;
use crate::core::types::Millis;
use crate::draw::clear_highlight_cache;
use crate::types::ScreenCell;
use nvim_oxi::{Result, api};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum IngressDispatchOutcome {
    Applied,
    Coalesced,
    Dropped,
}

fn build_cursor_autocmd_events(
    ingress: AutocmdIngress,
    observed_at: Millis,
    needs_initialize: bool,
    ingress_cursor_presentation: Option<IngressCursorPresentationRequest>,
) -> Vec<CoreEvent> {
    let should_request_observation = should_request_observation_for_autocmd(ingress);
    let mut events =
        Vec::with_capacity(usize::from(needs_initialize) + usize::from(should_request_observation));

    if needs_initialize {
        events.push(CoreEvent::Initialize(InitializeEvent { observed_at }));
    }

    if should_request_observation {
        let kind = demand_kind_for_autocmd(ingress);
        events.push(CoreEvent::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind,
            observed_at,
            requested_target: None,
            ingress_cursor_presentation: if kind.is_cursor() {
                ingress_cursor_presentation
            } else {
                None
            },
        }));
    }

    events
}

fn demand_kind_for_autocmd(ingress: AutocmdIngress) -> ExternalDemandKind {
    match ingress {
        AutocmdIngress::ModeChanged => ExternalDemandKind::ModeChanged,
        AutocmdIngress::BufEnter => ExternalDemandKind::BufferEntered,
        _ => ExternalDemandKind::ExternalCursor,
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

fn should_coalesce_window_follow_up_autocmd(
    ingress: AutocmdIngress,
    snapshot: &IngressReadSnapshot,
    current_window_handle: i64,
) -> bool {
    if ingress != AutocmdIngress::BufEnter {
        return false;
    }

    // Surprising: a window switch into a different buffer can emit `WinEnter` followed by
    // `BufEnter` before runtime tracking updates. In that sequence the window change is already
    // authoritative, so replaying a second surface observation from `BufEnter` just adds churn.
    snapshot
        .tracked_location()
        .is_some_and(|tracked| tracked.window_handle != current_window_handle)
}

fn on_cursor_event_core_for_autocmd(ingress: AutocmdIngress) -> Result<IngressDispatchOutcome> {
    let snapshot = ingress_read_snapshot()?;
    if !snapshot.enabled() {
        return Ok(IngressDispatchOutcome::Dropped);
    }

    let window = api::get_current_win();
    let buffer = api::get_current_buf();
    if !window.is_valid() || !buffer.is_valid() {
        return Ok(IngressDispatchOutcome::Dropped);
    }
    match skip_current_buffer_events(&snapshot, &buffer) {
        Ok(true) => return Ok(IngressDispatchOutcome::Dropped),
        Ok(false) => {}
        Err(err) => {
            warn(&format!("core cursor buffer policy failed: {err}"));
            return Ok(IngressDispatchOutcome::Dropped);
        }
    }
    note_autocmd_event_now();
    if should_coalesce_window_follow_up_autocmd(ingress, &snapshot, i64::from(window.handle())) {
        return Ok(IngressDispatchOutcome::Coalesced);
    }

    let ingress_cursor_presentation = if demand_kind_for_autocmd(ingress).is_cursor() {
        match collect_ingress_cursor_presentation_request(&snapshot) {
            Ok(request) => Some(request),
            Err(err) => {
                warn(&format!(
                    "core cursor presentation policy probe failed; continuing without prepaint: {err}"
                ));
                None
            }
        }
    } else {
        None
    };

    let observed_at = to_core_millis(now_ms());
    let events = build_cursor_autocmd_events(
        ingress,
        observed_at,
        snapshot.needs_initialize(),
        ingress_cursor_presentation,
    );

    if events.is_empty() {
        return Ok(IngressDispatchOutcome::Dropped);
    }

    dispatch_core_events_with_default_scheduler(events)?;
    Ok(IngressDispatchOutcome::Applied)
}

fn on_colorscheme_impl() -> Result<IngressDispatchOutcome> {
    clear_highlight_cache();
    note_cursor_color_colorscheme_change()?;
    Ok(IngressDispatchOutcome::Applied)
}

fn on_autocmd_ingress(ingress: AutocmdIngress) -> Result<IngressDispatchOutcome> {
    if ingress.is_colorscheme() {
        return on_colorscheme_impl();
    }
    if ingress == AutocmdIngress::Unknown {
        return Ok(IngressDispatchOutcome::Dropped);
    }

    on_cursor_event_core_for_autocmd(ingress)
}

fn dispatch_ingress(ingress: Ingress) -> Result<()> {
    record_ingress_received();
    let outcome = match ingress {
        Ingress::Autocmd(autocmd_ingress) => on_autocmd_ingress(autocmd_ingress),
    };
    match outcome {
        Ok(dispatch_outcome) => {
            match dispatch_outcome {
                IngressDispatchOutcome::Applied => record_ingress_applied(),
                IngressDispatchOutcome::Coalesced => {
                    record_ingress_applied();
                    record_ingress_coalesced();
                }
                IngressDispatchOutcome::Dropped => record_ingress_dropped(),
            }
            Ok(())
        }
        Err(err) => {
            record_ingress_dropped();
            Err(err)
        }
    }
}

pub(crate) fn on_autocmd_event(event: &str) -> Result<()> {
    dispatch_ingress(Ingress::Autocmd(parse_autocmd_ingress(event)))
}

#[cfg(test)]
mod tests {
    use super::{build_cursor_autocmd_events, should_coalesce_window_follow_up_autocmd};
    use crate::core::effect::IngressCursorPresentationRequest;
    use crate::core::event::{Event as CoreEvent, ExternalDemandQueuedEvent, InitializeEvent};
    use crate::core::state::ExternalDemandKind;
    use crate::core::types::Millis;
    use crate::events::ingress::AutocmdIngress;
    use crate::events::runtime::IngressReadSnapshot;
    use crate::state::CursorLocation;
    use crate::types::Point;

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
            Some(presentation()),
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
    fn cursor_autocmd_builder_uses_observation_only_routing_for_mode_change() {
        let observed_at = Millis::new(21);

        let events = build_cursor_autocmd_events(
            AutocmdIngress::ModeChanged,
            observed_at,
            false,
            Some(presentation()),
        );

        assert_eq!(
            events,
            vec![CoreEvent::ExternalDemandQueued(ExternalDemandQueuedEvent {
                kind: ExternalDemandKind::ModeChanged,
                observed_at,
                requested_target: None,
                ingress_cursor_presentation: None,
            })]
        );
    }

    #[test]
    fn cursor_autocmd_builder_uses_observation_only_routing_for_buffer_enter() {
        let observed_at = Millis::new(21);

        let events = build_cursor_autocmd_events(
            AutocmdIngress::BufEnter,
            observed_at,
            false,
            Some(presentation()),
        );

        assert_eq!(
            events,
            vec![CoreEvent::ExternalDemandQueued(ExternalDemandQueuedEvent {
                kind: ExternalDemandKind::BufferEntered,
                observed_at,
                requested_target: None,
                ingress_cursor_presentation: None,
            })]
        );
    }

    #[test]
    fn cursor_autocmd_builder_keeps_observation_when_presentation_probe_fails() {
        let observed_at = Millis::new(21);

        let events =
            build_cursor_autocmd_events(AutocmdIngress::CursorMoved, observed_at, false, None);

        assert_eq!(
            events,
            vec![CoreEvent::ExternalDemandQueued(ExternalDemandQueuedEvent {
                kind: ExternalDemandKind::ExternalCursor,
                observed_at,
                requested_target: None,
                ingress_cursor_presentation: None,
            })]
        );
    }

    #[test]
    fn buf_enter_is_coalesced_when_a_window_switch_is_still_pending() {
        let snapshot = IngressReadSnapshot::new_for_test(
            true,
            false,
            [Point::ZERO; 4],
            Some(CursorLocation::new(11, 22, 3, 4)),
            (true, true, true, true),
            Vec::new(),
        );

        assert!(should_coalesce_window_follow_up_autocmd(
            AutocmdIngress::BufEnter,
            &snapshot,
            33,
        ));
    }

    #[test]
    fn buf_enter_is_not_coalesced_for_same_window_buffer_switches() {
        let snapshot = IngressReadSnapshot::new_for_test(
            true,
            false,
            [Point::ZERO; 4],
            Some(CursorLocation::new(11, 22, 3, 4)),
            (true, true, true, true),
            Vec::new(),
        );

        assert!(!should_coalesce_window_follow_up_autocmd(
            AutocmdIngress::BufEnter,
            &snapshot,
            11,
        ));
    }

    #[test]
    fn non_buf_enter_autocmds_keep_their_observation_path() {
        let snapshot = IngressReadSnapshot::new_for_test(
            true,
            false,
            [Point::ZERO; 4],
            Some(CursorLocation::new(11, 22, 3, 4)),
            (true, true, true, true),
            Vec::new(),
        );

        assert!(!should_coalesce_window_follow_up_autocmd(
            AutocmdIngress::WinEnter,
            &snapshot,
            33,
        ));
    }
}

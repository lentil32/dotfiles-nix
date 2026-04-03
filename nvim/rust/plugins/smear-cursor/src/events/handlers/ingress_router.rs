use super::super::cursor::mode_string;
use super::super::cursor::smear_outside_cmd_row;
use super::super::ingress::AutocmdIngress;
use super::super::ingress::Ingress;
use super::super::ingress::parse_autocmd_ingress;
use super::super::logging::warn;
use super::super::runtime::IngressReadSnapshot;
use super::super::runtime::ingress_read_snapshot;
use super::super::runtime::note_autocmd_event_now;
use super::super::runtime::note_cursor_color_colorscheme_change;
use super::super::runtime::now_ms;
use super::super::runtime::record_ingress_applied;
use super::super::runtime::record_ingress_coalesced;
use super::super::runtime::record_ingress_dropped;
use super::super::runtime::record_ingress_received;
use super::super::runtime::to_core_millis;
use super::core_dispatch::dispatch_core_events_with_default_scheduler;
use super::source_selection::should_request_observation_for_autocmd;
use crate::core::effect::IngressCursorPresentationRequest;
use crate::core::event::Event as CoreEvent;
use crate::core::event::ExternalDemandQueuedEvent;
use crate::core::event::InitializeEvent;
use crate::core::state::BufferPerfClass;
use crate::core::state::ExternalDemandKind;
use crate::core::types::Millis;
use crate::draw::clear_highlight_cache;
use nvim_oxi::Result;
use nvim_oxi::api;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum IngressDispatchOutcome {
    Applied,
    Coalesced,
    Dropped,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum CursorAutocmdPreflight {
    Dropped,
    MissingPerfClass,
    Continue { buffer_perf_class: BufferPerfClass },
}

fn build_cursor_autocmd_events(
    ingress: AutocmdIngress,
    observed_at: Millis,
    needs_initialize: bool,
    buffer_perf_class: BufferPerfClass,
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
            buffer_perf_class,
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
        snapshot.current_visual_cursor_cell(),
        snapshot.current_visual_cursor_shape(),
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

fn cursor_autocmd_preflight(
    snapshot: &IngressReadSnapshot,
    window_valid: bool,
    buffer_valid: bool,
) -> CursorAutocmdPreflight {
    if !snapshot.enabled() || !window_valid || !buffer_valid {
        return CursorAutocmdPreflight::Dropped;
    }

    match snapshot.current_buffer_perf_class() {
        Some(BufferPerfClass::Skip) => CursorAutocmdPreflight::Dropped,
        Some(buffer_perf_class) => CursorAutocmdPreflight::Continue { buffer_perf_class },
        None => CursorAutocmdPreflight::MissingPerfClass,
    }
}

fn on_cursor_event_core_for_autocmd(ingress: AutocmdIngress) -> Result<IngressDispatchOutcome> {
    let snapshot = ingress_read_snapshot()?;
    let window = api::get_current_win();
    let buffer = api::get_current_buf();
    let buffer_perf_class =
        match cursor_autocmd_preflight(&snapshot, window.is_valid(), buffer.is_valid()) {
            CursorAutocmdPreflight::Dropped => return Ok(IngressDispatchOutcome::Dropped),
            CursorAutocmdPreflight::MissingPerfClass => {
                warn("core cursor buffer policy snapshot missing perf class");
                return Ok(IngressDispatchOutcome::Dropped);
            }
            CursorAutocmdPreflight::Continue { buffer_perf_class } => buffer_perf_class,
        };
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
        buffer_perf_class,
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
    use super::CursorAutocmdPreflight;
    use super::build_cursor_autocmd_events;
    use super::cursor_autocmd_preflight;
    use super::demand_kind_for_autocmd;
    use super::should_coalesce_window_follow_up_autocmd;
    use crate::config::BufferPerfMode;
    use crate::core::effect::IngressCursorPresentationRequest;
    use crate::core::event::Event as CoreEvent;
    use crate::core::event::ExternalDemandQueuedEvent;
    use crate::core::event::InitializeEvent;
    use crate::core::state::BufferPerfClass;
    use crate::core::types::Millis;
    use crate::events::ingress::AutocmdIngress;
    use crate::events::runtime::IngressReadSnapshot;
    use crate::events::runtime::IngressReadSnapshotTestInput;
    use crate::state::CursorLocation;
    use crate::test_support::proptest::pure_config;
    use crate::types::CursorCellShape;
    use crate::types::Point;
    use proptest::prelude::*;

    fn presentation() -> IngressCursorPresentationRequest {
        IngressCursorPresentationRequest::new(true, true, None, CursorCellShape::Block)
    }

    fn autocmd_ingress_strategy() -> BoxedStrategy<AutocmdIngress> {
        prop_oneof![
            Just(AutocmdIngress::CmdlineChanged),
            Just(AutocmdIngress::CursorMoved),
            Just(AutocmdIngress::CursorMovedInsert),
            Just(AutocmdIngress::ModeChanged),
            Just(AutocmdIngress::WinEnter),
            Just(AutocmdIngress::WinScrolled),
            Just(AutocmdIngress::BufEnter),
        ]
        .boxed()
    }

    fn perf_class_strategy() -> BoxedStrategy<Option<BufferPerfClass>> {
        prop_oneof![
            Just(None),
            Just(Some(BufferPerfClass::Full)),
            Just(Some(BufferPerfClass::FastMotion)),
            Just(Some(BufferPerfClass::Skip)),
        ]
        .boxed()
    }

    fn snapshot_with_state(
        enabled: bool,
        buffer_perf_class: Option<BufferPerfClass>,
        tracked_location: Option<CursorLocation>,
    ) -> IngressReadSnapshot {
        IngressReadSnapshot::new_for_test(IngressReadSnapshotTestInput {
            enabled,
            needs_initialize: false,
            current_corners: [Point::ZERO; 4],
            target_corners: [Point::ZERO; 4],
            target_position: Point::ZERO,
            tracked_location,
            mode_flags: [true, true, true, true],
            buffer_perf_mode: BufferPerfMode::Auto,
            callback_duration_estimate_ms: 0.0,
            current_buffer_perf_class: buffer_perf_class,
            filetypes_disabled: Vec::new(),
        })
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_cursor_autocmd_builder_matches_routing_and_presentation_rules(
            ingress in autocmd_ingress_strategy(),
            observed_at in any::<u64>(),
            needs_initialize in any::<bool>(),
            buffer_perf_class in prop_oneof![
                Just(BufferPerfClass::Full),
                Just(BufferPerfClass::FastMotion),
                Just(BufferPerfClass::Skip),
            ],
            include_presentation in any::<bool>(),
        ) {
            let observed_at = Millis::new(observed_at);
            let ingress_cursor_presentation = include_presentation.then(presentation);
            let kind = demand_kind_for_autocmd(ingress);
            let mut expected = Vec::new();

            if needs_initialize {
                expected.push(CoreEvent::Initialize(InitializeEvent { observed_at }));
            }
            expected.push(CoreEvent::ExternalDemandQueued(ExternalDemandQueuedEvent {
                kind,
                observed_at,
                requested_target: None,
                buffer_perf_class,
                ingress_cursor_presentation: if kind.is_cursor() {
                    ingress_cursor_presentation
                } else {
                    None
                },
            }));

            prop_assert_eq!(
                build_cursor_autocmd_events(
                    ingress,
                    observed_at,
                    needs_initialize,
                    buffer_perf_class,
                    ingress_cursor_presentation,
                ),
                expected
            );
        }

        #[test]
        fn prop_cursor_autocmd_preflight_matches_enable_validity_and_perf_class_gates(
            enabled in any::<bool>(),
            window_valid in any::<bool>(),
            buffer_valid in any::<bool>(),
            buffer_perf_class in perf_class_strategy(),
        ) {
            let snapshot = snapshot_with_state(enabled, buffer_perf_class, None);
            let expected = if !enabled || !window_valid || !buffer_valid {
                CursorAutocmdPreflight::Dropped
            } else {
                match buffer_perf_class {
                    Some(BufferPerfClass::Skip) => CursorAutocmdPreflight::Dropped,
                    Some(buffer_perf_class) => CursorAutocmdPreflight::Continue { buffer_perf_class },
                    None => CursorAutocmdPreflight::MissingPerfClass,
                }
            };

            prop_assert_eq!(
                cursor_autocmd_preflight(&snapshot, window_valid, buffer_valid),
                expected
            );
        }

        #[test]
        fn prop_window_follow_up_coalescing_depends_only_on_buf_enter_and_window_change(
            ingress in autocmd_ingress_strategy(),
            tracked_window_handle in 1_i64..=64_i64,
            current_window_handle in 1_i64..=64_i64,
            tracked_location_present in any::<bool>(),
        ) {
            let tracked_location = tracked_location_present
                .then(|| CursorLocation::new(tracked_window_handle, 22, 3, 4));
            let snapshot = snapshot_with_state(
                true,
                Some(BufferPerfClass::Full),
                tracked_location,
            );
            let expected = ingress == AutocmdIngress::BufEnter
                && tracked_location_present
                && tracked_window_handle != current_window_handle;

            prop_assert_eq!(
                should_coalesce_window_follow_up_autocmd(
                    ingress,
                    &snapshot,
                    current_window_handle,
                ),
                expected
            );
        }
    }
}

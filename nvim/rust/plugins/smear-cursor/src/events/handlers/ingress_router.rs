use super::super::cursor::current_mode;
use super::super::cursor::cursor_position_for_mode;
use super::super::cursor::smear_outside_cmd_row;
use super::super::ingress::AutocmdIngress;
use super::super::ingress::Ingress;
use super::super::ingress::parse_autocmd_ingress;
use super::super::logging::warn;
use super::super::runtime::IngressReadSnapshot;
use super::super::runtime::ingress_read_snapshot;
use super::super::runtime::mutate_engine_state;
use super::super::runtime::note_autocmd_event_now;
use super::super::runtime::note_cursor_color_colorscheme_change;
use super::super::runtime::now_ms;
use super::super::runtime::read_engine_state;
use super::super::runtime::record_ingress_applied;
use super::super::runtime::record_ingress_coalesced;
use super::super::runtime::record_ingress_dropped;
use super::super::runtime::record_ingress_received;
use super::super::runtime::refresh_editor_viewport_cache;
use super::super::runtime::to_core_millis;
use super::core_dispatch::dispatch_core_events_with_default_scheduler;
use super::source_selection::should_request_observation_for_autocmd;
use super::viewport::cursor_location_for_ingress_fast_path;
use crate::core::effect::IngressCursorPresentationRequest;
use crate::core::event::Event as CoreEvent;
use crate::core::event::ExternalDemandQueuedEvent;
use crate::core::event::InitializeEvent;
use crate::core::state::BufferPerfClass;
use crate::core::state::ExternalDemandKind;
use crate::core::types::Millis;
use crate::draw::clear_highlight_cache;
use crate::state::CursorLocation;
use crate::types::EPSILON;
use crate::types::Point;
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::types::AutocmdCallbackArgs;

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

#[derive(Debug, Clone, PartialEq)]
struct CursorAutocmdFastPathSnapshot {
    enabled: bool,
    needs_initialize: bool,
    tracked_location: Option<CursorLocation>,
    target_position: Point,
    smear_to_cmd: bool,
}

fn cursor_autocmd_fast_path_snapshot()
-> super::super::runtime::EngineAccessResult<CursorAutocmdFastPathSnapshot> {
    read_engine_state(|state| {
        let runtime = state.core_state().runtime();
        CursorAutocmdFastPathSnapshot {
            enabled: runtime.is_enabled(),
            needs_initialize: state.core_state().needs_initialize(),
            tracked_location: runtime.tracked_location(),
            target_position: runtime.target_position(),
            smear_to_cmd: runtime.config.smear_to_cmd,
        }
    })
}

fn current_cursor_position_for_fast_path(
    window: &api::Window,
    smear_to_cmd: bool,
) -> Option<Point> {
    let mode = current_mode();
    let mode = mode.to_string_lossy();
    cursor_position_for_mode(window, mode.as_ref(), smear_to_cmd)
        .ok()
        .flatten()
        .map(|(row, col)| Point { row, col })
}

fn should_drop_unchanged_cursor_autocmd(
    ingress: AutocmdIngress,
    snapshot: &CursorAutocmdFastPathSnapshot,
    current_location: Option<&CursorLocation>,
    current_target_position: Option<Point>,
) -> bool {
    if !ingress.supports_unchanged_fast_path() || !snapshot.enabled || snapshot.needs_initialize {
        return false;
    }

    let (Some(tracked_location), Some(current_location), Some(current_target_position)) = (
        snapshot.tracked_location.as_ref(),
        current_location,
        current_target_position,
    ) else {
        return false;
    };

    tracked_location == current_location
        && snapshot
            .target_position
            .distance_squared(current_target_position)
            <= EPSILON
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
    let mode = current_mode();
    let mode = mode.to_string_lossy();
    let current_corners = snapshot.current_corners();
    let mode_allowed = snapshot.mode_allowed(mode.as_ref());

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

fn maybe_drop_unchanged_cursor_autocmd(
    ingress: AutocmdIngress,
) -> Result<Option<IngressDispatchOutcome>> {
    if !ingress.supports_unchanged_fast_path() {
        return Ok(None);
    }

    let fast_path_snapshot = cursor_autocmd_fast_path_snapshot()?;
    if !fast_path_snapshot.enabled
        || fast_path_snapshot.needs_initialize
        || fast_path_snapshot.tracked_location.is_none()
    {
        return Ok(None);
    }

    let Some(current_location) = cursor_location_for_ingress_fast_path() else {
        return Ok(None);
    };
    let window = api::get_current_win();
    if !window.is_valid() {
        return Ok(None);
    }
    let current_target_position =
        current_cursor_position_for_fast_path(&window, fast_path_snapshot.smear_to_cmd);

    if should_drop_unchanged_cursor_autocmd(
        ingress,
        &fast_path_snapshot,
        Some(&current_location),
        current_target_position,
    ) {
        note_autocmd_event_now();
        return Ok(Some(IngressDispatchOutcome::Dropped));
    }

    Ok(None)
}

fn on_cursor_event_core_for_autocmd(ingress: AutocmdIngress) -> Result<IngressDispatchOutcome> {
    if let Some(outcome) = maybe_drop_unchanged_cursor_autocmd(ingress)? {
        return Ok(outcome);
    }
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

fn should_invalidate_buffer_metadata_for_option(option_name: &str) -> bool {
    matches!(option_name, "filetype" | "buftype" | "buflisted")
}

fn should_refresh_editor_viewport_for_option(option_name: &str) -> bool {
    matches!(option_name, "cmdheight" | "lines" | "columns")
}

fn invalidate_buffer_metadata_for_option_set(args: &AutocmdCallbackArgs) -> Result<()> {
    if !should_invalidate_buffer_metadata_for_option(args.r#match.as_str()) {
        return Ok(());
    }

    let buffer_handle = i64::from(args.buffer.handle());
    mutate_engine_state(|state| {
        state
            .shell
            .buffer_metadata_cache
            .invalidate_buffer(buffer_handle);
    })
    .map_err(nvim_oxi::Error::from)?;
    Ok(())
}

fn refresh_editor_viewport_for_option_set(args: &AutocmdCallbackArgs) -> Result<()> {
    if !should_refresh_editor_viewport_for_option(args.r#match.as_str()) {
        return Ok(());
    }

    refresh_editor_viewport_cache()
}

fn invalidate_buffer_local_caches(buffer_handle: i64) -> Result<()> {
    mutate_engine_state(|state| {
        state
            .shell
            .buffer_metadata_cache
            .invalidate_buffer(buffer_handle);
        state
            .shell
            .buffer_perf_policy_cache
            .invalidate_buffer(buffer_handle);
        state
            .shell
            .buffer_perf_telemetry_cache
            .invalidate_buffer(buffer_handle);
    })
    .map_err(nvim_oxi::Error::from)?;
    Ok(())
}

fn on_autocmd_ingress(
    ingress: AutocmdIngress,
    args: Option<&AutocmdCallbackArgs>,
) -> Result<IngressDispatchOutcome> {
    if ingress.is_colorscheme() {
        return on_colorscheme_impl();
    }

    match ingress {
        AutocmdIngress::BufWipeout => {
            if let Some(args) = args {
                invalidate_buffer_local_caches(i64::from(args.buffer.handle()))?;
            }
            Ok(IngressDispatchOutcome::Dropped)
        }
        AutocmdIngress::OptionSet => {
            if let Some(args) = args {
                invalidate_buffer_metadata_for_option_set(args)?;
                refresh_editor_viewport_for_option_set(args)?;
            }
            Ok(IngressDispatchOutcome::Dropped)
        }
        AutocmdIngress::VimResized => {
            refresh_editor_viewport_cache()?;
            Ok(IngressDispatchOutcome::Dropped)
        }
        AutocmdIngress::Unknown => Ok(IngressDispatchOutcome::Dropped),
        _ => on_cursor_event_core_for_autocmd(ingress),
    }
}

fn dispatch_ingress(ingress: Ingress, args: Option<&AutocmdCallbackArgs>) -> Result<()> {
    record_ingress_received();
    let outcome = match ingress {
        Ingress::Autocmd(autocmd_ingress) => on_autocmd_ingress(autocmd_ingress, args),
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
    dispatch_ingress(Ingress::Autocmd(parse_autocmd_ingress(event)), None)
}

pub(in crate::events) fn on_autocmd_callback(args: AutocmdCallbackArgs) -> Result<()> {
    dispatch_ingress(
        Ingress::Autocmd(parse_autocmd_ingress(args.event.as_str())),
        Some(&args),
    )
}

#[cfg(test)]
mod tests {
    use super::CursorAutocmdFastPathSnapshot;
    use super::CursorAutocmdPreflight;
    use super::build_cursor_autocmd_events;
    use super::cursor_autocmd_preflight;
    use super::demand_kind_for_autocmd;
    use super::should_coalesce_window_follow_up_autocmd;
    use super::should_drop_unchanged_cursor_autocmd;
    use super::should_invalidate_buffer_metadata_for_option;
    use super::should_refresh_editor_viewport_for_option;
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
    use pretty_assertions::assert_eq;
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

    fn fast_path_snapshot(
        enabled: bool,
        needs_initialize: bool,
        tracked_location: Option<CursorLocation>,
        target_position: Point,
    ) -> CursorAutocmdFastPathSnapshot {
        CursorAutocmdFastPathSnapshot {
            enabled,
            needs_initialize,
            tracked_location,
            target_position,
            smear_to_cmd: true,
        }
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

    #[test]
    fn buffer_metadata_invalidation_only_tracks_the_buffer_local_policy_inputs() {
        for (option_name, expected) in [
            ("filetype", true),
            ("buftype", true),
            ("buflisted", true),
            ("conceallevel", false),
            ("number", false),
        ] {
            assert_eq!(
                should_invalidate_buffer_metadata_for_option(option_name),
                expected,
                "unexpected invalidation result for {option_name}"
            );
        }
    }

    #[test]
    fn editor_viewport_refresh_tracks_only_global_viewport_inputs() {
        for (option_name, expected) in [
            ("cmdheight", true),
            ("lines", true),
            ("columns", true),
            ("filetype", false),
            ("number", false),
        ] {
            assert_eq!(
                should_refresh_editor_viewport_for_option(option_name),
                expected,
                "unexpected viewport refresh result for {option_name}"
            );
        }
    }

    #[test]
    fn unchanged_cursor_fast_path_requires_matching_surface_and_target() {
        let tracked_location = CursorLocation::new(10, 20, 4, 12)
            .with_viewport_columns(3, 1)
            .with_window_origin(5, 7)
            .with_window_dimensions(80, 24);
        let matching_target = Point {
            row: 11.0,
            col: 22.0,
        };
        let matching_snapshot =
            fast_path_snapshot(true, false, Some(tracked_location.clone()), matching_target);

        for (label, ingress, snapshot, current_location, current_target, expected) in [
            (
                "cursor moved repeat",
                AutocmdIngress::CursorMoved,
                matching_snapshot.clone(),
                Some(tracked_location.clone()),
                Some(matching_target),
                true,
            ),
            (
                "insert repeat",
                AutocmdIngress::CursorMovedInsert,
                matching_snapshot.clone(),
                Some(tracked_location.clone()),
                Some(matching_target),
                true,
            ),
            (
                "window scrolled repeat",
                AutocmdIngress::WinScrolled,
                matching_snapshot.clone(),
                Some(tracked_location.clone()),
                Some(matching_target),
                true,
            ),
            (
                "window enter repeat",
                AutocmdIngress::WinEnter,
                matching_snapshot.clone(),
                Some(tracked_location.clone()),
                Some(matching_target),
                true,
            ),
            (
                "buffer enter repeat",
                AutocmdIngress::BufEnter,
                matching_snapshot.clone(),
                Some(tracked_location.clone()),
                Some(matching_target),
                true,
            ),
            (
                "mode changes still require full path",
                AutocmdIngress::ModeChanged,
                matching_snapshot.clone(),
                Some(tracked_location.clone()),
                Some(matching_target),
                false,
            ),
            (
                "surface changes stay live",
                AutocmdIngress::CursorMoved,
                matching_snapshot.clone(),
                Some(CursorLocation::new(10, 20, 4, 13)),
                Some(matching_target),
                false,
            ),
            (
                "target changes stay live",
                AutocmdIngress::CursorMoved,
                matching_snapshot.clone(),
                Some(tracked_location.clone()),
                Some(Point {
                    row: 12.0,
                    col: 22.0,
                }),
                false,
            ),
            (
                "missing live position disables the fast path",
                AutocmdIngress::CursorMoved,
                matching_snapshot,
                Some(tracked_location.clone()),
                None,
                false,
            ),
            (
                "uninitialized runtime disables the fast path",
                AutocmdIngress::CursorMoved,
                fast_path_snapshot(true, true, Some(tracked_location.clone()), matching_target),
                Some(tracked_location.clone()),
                Some(matching_target),
                false,
            ),
            (
                "disabled runtime disables the fast path",
                AutocmdIngress::CursorMoved,
                fast_path_snapshot(
                    false,
                    false,
                    Some(tracked_location.clone()),
                    matching_target,
                ),
                Some(tracked_location.clone()),
                Some(matching_target),
                false,
            ),
            (
                "missing tracked location disables the fast path",
                AutocmdIngress::CursorMoved,
                fast_path_snapshot(true, false, None, matching_target),
                Some(tracked_location),
                Some(matching_target),
                false,
            ),
        ] {
            assert_eq!(
                should_drop_unchanged_cursor_autocmd(
                    ingress,
                    &snapshot,
                    current_location.as_ref(),
                    current_target,
                ),
                expected,
                "{label}"
            );
        }
    }
}

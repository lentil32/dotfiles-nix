use super::super::cursor::current_mode;
use super::super::cursor::cursor_observation_for_mode_with_probe_policy;
use super::super::cursor::smear_outside_cmd_row;
use super::super::ingress::AutocmdIngress;
use super::super::ingress::Ingress;
use super::super::ingress::parse_autocmd_ingress;
use super::super::logging::warn;
use super::super::runtime::IngressReadSnapshot;
use super::super::runtime::ingress_read_snapshot_with_current_buffer;
use super::super::runtime::mutate_engine_state;
use super::super::runtime::note_autocmd_event_now;
use super::super::runtime::note_cursor_color_colorscheme_change;
use super::super::runtime::now_ms;
use super::super::runtime::read_engine_state;
use super::super::runtime::record_cursor_autocmd_fast_path_continued;
use super::super::runtime::record_cursor_autocmd_fast_path_dropped;
use super::super::runtime::record_ingress_applied;
use super::super::runtime::record_ingress_coalesced;
use super::super::runtime::record_ingress_dropped;
use super::super::runtime::record_ingress_received;
use super::super::runtime::refresh_editor_viewport_cache;
use super::super::runtime::to_core_millis;
use super::super::surface::current_window_surface_snapshot;
use super::core_dispatch::dispatch_core_events_with_default_scheduler;
use super::source_selection::should_request_observation_for_autocmd;
use super::viewport::surface_for_ingress_fast_path_with_handles;
use crate::core::effect::IngressCursorPresentationRequest;
use crate::core::effect::IngressObservationSurface;
use crate::core::effect::ProbePolicy;
use crate::core::event::Event as CoreEvent;
use crate::core::event::ExternalDemandQueuedEvent;
use crate::core::event::InitializeEvent;
use crate::core::state::BufferPerfClass;
use crate::core::state::ExternalDemandKind;
use crate::core::types::Millis;
use crate::draw::clear_highlight_cache;
use crate::position::CursorObservation;
use crate::position::RenderPoint;
use crate::position::WindowSurfaceSnapshot;
use crate::state::TrackedCursor;
use crate::types::EPSILON;
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

#[derive(Debug)]
enum CursorAutocmdFastPathResult {
    Dropped,
    Continue {
        current_surface: Option<WindowSurfaceSnapshot>,
        current_cursor: Option<CursorObservation>,
        window: api::Window,
        buffer: api::Buffer,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum CursorAutocmdFastPathOutcome {
    Dropped,
    Continue,
}

#[derive(Debug, Clone, PartialEq)]
struct CursorAutocmdFastPathSnapshot {
    enabled: bool,
    needs_initialize: bool,
    tracked_cursor: Option<TrackedCursor>,
    target_position: RenderPoint,
    smear_to_cmd: bool,
}

fn cursor_autocmd_fast_path_snapshot()
-> super::super::runtime::EngineAccessResult<CursorAutocmdFastPathSnapshot> {
    read_engine_state(|state| {
        let runtime = state.core_state().runtime();
        CursorAutocmdFastPathSnapshot {
            enabled: runtime.is_enabled(),
            needs_initialize: state.core_state().needs_initialize(),
            tracked_cursor: runtime.tracked_cursor(),
            target_position: runtime.target_position(),
            smear_to_cmd: runtime.config.smear_to_cmd,
        }
    })
}

fn record_cursor_autocmd_fast_path_outcome(
    ingress: AutocmdIngress,
    outcome: CursorAutocmdFastPathOutcome,
) {
    match outcome {
        CursorAutocmdFastPathOutcome::Dropped => {
            record_cursor_autocmd_fast_path_dropped(ingress);
        }
        CursorAutocmdFastPathOutcome::Continue => {
            record_cursor_autocmd_fast_path_continued(ingress);
        }
    }
}

fn current_cursor_observation_for_fast_path(
    window: &api::Window,
    smear_to_cmd: bool,
    mode: &str,
    surface_snapshot: Option<&WindowSurfaceSnapshot>,
) -> Option<CursorObservation> {
    cursor_observation_for_mode_with_probe_policy(
        window,
        mode,
        smear_to_cmd,
        ProbePolicy::exact(),
        surface_snapshot,
    )
    .ok()
}

fn tracked_cursor_matches_live_surface_handles(
    tracked_cursor: &TrackedCursor,
    current_window_handle: i64,
    current_buffer_handle: i64,
) -> bool {
    tracked_cursor.window_handle() == current_window_handle
        && tracked_cursor.buffer_handle() == current_buffer_handle
}

fn should_drop_unchanged_cursor_autocmd(
    ingress: AutocmdIngress,
    snapshot: &CursorAutocmdFastPathSnapshot,
    current_tracked_cursor: Option<&TrackedCursor>,
    current_target_position: Option<RenderPoint>,
) -> bool {
    if !ingress.supports_unchanged_fast_path() || !snapshot.enabled || snapshot.needs_initialize {
        return false;
    }

    let (Some(tracked_cursor), Some(current_tracked_cursor), Some(current_target_position)) = (
        snapshot.tracked_cursor.as_ref(),
        current_tracked_cursor,
        current_target_position,
    ) else {
        return false;
    };

    tracked_cursor == current_tracked_cursor
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
    ingress_observation_surface: Option<IngressObservationSurface>,
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
            buffer_perf_class,
            ingress_cursor_presentation: if kind.is_cursor() {
                ingress_cursor_presentation
            } else {
                None
            },
            ingress_observation_surface,
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
    mode: &str,
) -> Result<IngressCursorPresentationRequest> {
    let current_corners = snapshot.current_corners();
    let mode_allowed = snapshot.mode_allowed(mode);

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
        .tracked_cursor()
        .is_some_and(|tracked| tracked.window_handle() != current_window_handle)
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
) -> Result<CursorAutocmdFastPathResult> {
    let window = api::get_current_win();
    let buffer = api::get_current_buf();

    if !ingress.supports_unchanged_fast_path() {
        let result = CursorAutocmdFastPathResult::Continue {
            current_surface: None,
            current_cursor: None,
            window,
            buffer,
        };
        record_cursor_autocmd_fast_path_outcome(ingress, CursorAutocmdFastPathOutcome::Continue);
        return Ok(result);
    }

    let fast_path_snapshot = cursor_autocmd_fast_path_snapshot()?;
    if !fast_path_snapshot.enabled || fast_path_snapshot.needs_initialize {
        let result = CursorAutocmdFastPathResult::Continue {
            current_surface: None,
            current_cursor: None,
            window,
            buffer,
        };
        record_cursor_autocmd_fast_path_outcome(ingress, CursorAutocmdFastPathOutcome::Continue);
        return Ok(result);
    }

    let Some(tracked_cursor) = fast_path_snapshot.tracked_cursor.as_ref() else {
        let result = CursorAutocmdFastPathResult::Continue {
            current_surface: None,
            current_cursor: None,
            window,
            buffer,
        };
        record_cursor_autocmd_fast_path_outcome(ingress, CursorAutocmdFastPathOutcome::Continue);
        return Ok(result);
    };
    if !window.is_valid() || i64::from(window.handle()) != tracked_cursor.window_handle() {
        let result = CursorAutocmdFastPathResult::Continue {
            current_surface: None,
            current_cursor: None,
            window,
            buffer,
        };
        record_cursor_autocmd_fast_path_outcome(ingress, CursorAutocmdFastPathOutcome::Continue);
        return Ok(result);
    }

    if !buffer.is_valid()
        || !tracked_cursor_matches_live_surface_handles(
            tracked_cursor,
            i64::from(window.handle()),
            i64::from(buffer.handle()),
        )
    {
        let result = CursorAutocmdFastPathResult::Continue {
            current_surface: None,
            current_cursor: None,
            window,
            buffer,
        };
        record_cursor_autocmd_fast_path_outcome(ingress, CursorAutocmdFastPathOutcome::Continue);
        return Ok(result);
    }

    let Some(current_surface) = surface_for_ingress_fast_path_with_handles(&window, &buffer) else {
        let result = CursorAutocmdFastPathResult::Continue {
            current_surface: None,
            current_cursor: None,
            window,
            buffer,
        };
        record_cursor_autocmd_fast_path_outcome(ingress, CursorAutocmdFastPathOutcome::Continue);
        return Ok(result);
    };
    let mode = current_mode();
    let mode = mode.to_string_lossy();
    let current_cursor = current_cursor_observation_for_fast_path(
        &window,
        fast_path_snapshot.smear_to_cmd,
        mode.as_ref(),
        Some(&current_surface),
    );
    let current_tracked_cursor =
        current_cursor.map(|cursor| TrackedCursor::new(current_surface, cursor.buffer_line()));
    let current_target_position = current_cursor
        .and_then(CursorObservation::screen_cell)
        .map(RenderPoint::from);

    if should_drop_unchanged_cursor_autocmd(
        ingress,
        &fast_path_snapshot,
        current_tracked_cursor.as_ref(),
        current_target_position,
    ) {
        note_autocmd_event_now();
        record_cursor_autocmd_fast_path_outcome(ingress, CursorAutocmdFastPathOutcome::Dropped);
        return Ok(CursorAutocmdFastPathResult::Dropped);
    }

    record_cursor_autocmd_fast_path_outcome(ingress, CursorAutocmdFastPathOutcome::Continue);
    Ok(CursorAutocmdFastPathResult::Continue {
        current_surface: Some(current_surface),
        current_cursor,
        window,
        buffer,
    })
}

fn ingress_observation_surface(
    window: &api::Window,
    buffer: &api::Buffer,
    current_surface: Option<WindowSurfaceSnapshot>,
    current_cursor: Option<CursorObservation>,
    mode: String,
) -> Option<IngressObservationSurface> {
    if !window.is_valid() || !buffer.is_valid() {
        return None;
    }

    let surface = match current_surface {
        Some(surface) => surface,
        None => current_window_surface_snapshot(window)
            .ok()
            .filter(|surface| surface.id().buffer_handle() == i64::from(buffer.handle()))?,
    };

    Some(IngressObservationSurface::new(
        surface,
        current_cursor,
        mode,
    ))
}

fn on_cursor_event_core_for_autocmd(ingress: AutocmdIngress) -> Result<IngressDispatchOutcome> {
    let (current_surface, current_cursor, window, buffer) =
        match maybe_drop_unchanged_cursor_autocmd(ingress)? {
            CursorAutocmdFastPathResult::Dropped => return Ok(IngressDispatchOutcome::Dropped),
            CursorAutocmdFastPathResult::Continue {
                current_surface,
                current_cursor,
                window,
                buffer,
            } => (current_surface, current_cursor, window, buffer),
        };
    let window_valid = window.is_valid();
    let buffer_valid = buffer.is_valid();
    let snapshot = ingress_read_snapshot_with_current_buffer(buffer_valid.then_some(&buffer))?;
    let buffer_perf_class = match cursor_autocmd_preflight(&snapshot, window_valid, buffer_valid) {
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
    let mode = current_mode().to_string_lossy().into_owned();
    let ingress_cursor_presentation = if demand_kind_for_autocmd(ingress).is_cursor() {
        match collect_ingress_cursor_presentation_request(&snapshot, &mode) {
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
    let ingress_observation_surface =
        ingress_observation_surface(&window, &buffer, current_surface, current_cursor, mode);

    let observed_at = to_core_millis(now_ms());
    let events = build_cursor_autocmd_events(
        ingress,
        observed_at,
        snapshot.needs_initialize(),
        buffer_perf_class,
        ingress_cursor_presentation,
        ingress_observation_surface,
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

fn should_invalidate_conceal_probe_cache_for_option(option_name: &str) -> bool {
    matches!(option_name, "conceallevel" | "concealcursor")
}

fn invalidate_buffer_metadata(buffer_handle: i64) -> Result<()> {
    mutate_engine_state(|state| {
        state.shell.invalidate_buffer_metadata(buffer_handle);
    })
    .map_err(nvim_oxi::Error::from)?;
    Ok(())
}

fn invalidate_buffer_metadata_for_option_set(args: &AutocmdCallbackArgs) -> Result<()> {
    if !should_invalidate_buffer_metadata_for_option(args.r#match.as_str()) {
        return Ok(());
    }

    invalidate_buffer_metadata(i64::from(args.buffer.handle()))
}

fn refresh_editor_viewport_for_option_set(args: &AutocmdCallbackArgs) -> Result<()> {
    if !should_refresh_editor_viewport_for_option(args.r#match.as_str()) {
        return Ok(());
    }

    refresh_editor_viewport_cache()
}

fn invalidate_buffer_local_probe_caches(buffer_handle: i64) -> Result<()> {
    mutate_engine_state(|state| {
        state
            .shell
            .invalidate_buffer_local_probe_caches(buffer_handle);
    })
    .map_err(nvim_oxi::Error::from)?;
    Ok(())
}

fn advance_buffer_text_revision(buffer_handle: i64) -> Result<()> {
    mutate_engine_state(|state| {
        state
            .shell
            .buffer_text_revision_cache
            .advance(buffer_handle);
    })
    .map_err(nvim_oxi::Error::from)?;
    Ok(())
}

fn invalidate_conceal_probe_caches(buffer_handle: i64) -> Result<()> {
    mutate_engine_state(|state| {
        state.shell.invalidate_conceal_probe_caches(buffer_handle);
    })
    .map_err(nvim_oxi::Error::from)?;
    Ok(())
}

fn invalidate_conceal_probe_caches_for_option_set(args: &AutocmdCallbackArgs) -> Result<()> {
    if !should_invalidate_conceal_probe_cache_for_option(args.r#match.as_str()) {
        return Ok(());
    }

    invalidate_conceal_probe_caches(i64::from(args.buffer.handle()))
}

fn invalidate_buffer_local_caches(buffer_handle: i64) -> Result<()> {
    mutate_engine_state(|state| {
        state.shell.invalidate_buffer_local_caches(buffer_handle);
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
                invalidate_conceal_probe_caches_for_option_set(args)?;
                refresh_editor_viewport_for_option_set(args)?;
            }
            Ok(IngressDispatchOutcome::Dropped)
        }
        AutocmdIngress::TextChanged | AutocmdIngress::TextChangedInsert => {
            if let Some(args) = args {
                let buffer_handle = i64::from(args.buffer.handle());
                advance_buffer_text_revision(buffer_handle)?;
                invalidate_buffer_metadata(buffer_handle)?;
                invalidate_buffer_local_probe_caches(buffer_handle)?;
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
    use super::advance_buffer_text_revision;
    use super::build_cursor_autocmd_events;
    use super::cursor_autocmd_preflight;
    use super::demand_kind_for_autocmd;
    use super::invalidate_buffer_local_caches;
    use super::invalidate_buffer_local_probe_caches;
    use super::invalidate_buffer_metadata;
    use super::invalidate_conceal_probe_caches;
    use super::should_coalesce_window_follow_up_autocmd;
    use super::should_drop_unchanged_cursor_autocmd;
    use super::should_invalidate_buffer_metadata_for_option;
    use super::should_invalidate_conceal_probe_cache_for_option;
    use super::should_refresh_editor_viewport_for_option;
    use super::tracked_cursor_matches_live_surface_handles;
    use crate::config::BufferPerfMode;
    use crate::core::effect::IngressCursorPresentationRequest;
    use crate::core::event::Event as CoreEvent;
    use crate::core::event::ExternalDemandQueuedEvent;
    use crate::core::event::InitializeEvent;
    use crate::core::state::BufferPerfClass;
    use crate::core::state::CursorTextContext;
    use crate::core::types::Generation;
    use crate::core::types::Millis;
    use crate::events::cursor::BufferMetadata;
    use crate::events::handlers::should_request_observation_for_autocmd;
    use crate::events::ingress::AutocmdIngress;
    use crate::events::policy::BufferEventPolicy;
    use crate::events::probe_cache::CachedConcealRegions;
    use crate::events::probe_cache::ConcealCacheLookup;
    use crate::events::probe_cache::CursorTextContextCacheKey;
    use crate::events::probe_cache::CursorTextContextCacheLookup;
    use crate::events::runtime::IngressReadSnapshot;
    use crate::events::runtime::IngressReadSnapshotTestInput;
    use crate::events::runtime::mutate_engine_state;
    use crate::events::runtime::read_engine_state;
    use crate::position::RenderPoint;
    use crate::state::TrackedCursor;
    use crate::test_support::conceal_key;
    use crate::test_support::conceal_region;
    use crate::test_support::proptest::pure_config;
    use crate::types::CursorCellShape;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::sync::Arc;

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
        tracked_cursor: Option<TrackedCursor>,
    ) -> IngressReadSnapshot {
        IngressReadSnapshot::new_for_test(IngressReadSnapshotTestInput {
            enabled,
            needs_initialize: false,
            current_corners: [RenderPoint::ZERO; 4],
            target_corners: [RenderPoint::ZERO; 4],
            target_position: RenderPoint::ZERO,
            tracked_cursor,
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
        tracked_cursor: Option<TrackedCursor>,
        target_position: RenderPoint,
    ) -> CursorAutocmdFastPathSnapshot {
        CursorAutocmdFastPathSnapshot {
            enabled,
            needs_initialize,
            tracked_cursor,
            target_position,
            smear_to_cmd: true,
        }
    }

    fn reset_buffer_local_cache_state() {
        mutate_engine_state(|state| {
            state.shell.reset_transient_caches();
        })
        .expect("engine state access should succeed");
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
                buffer_perf_class,
                ingress_cursor_presentation: if kind.is_cursor() {
                    ingress_cursor_presentation
                } else {
                    None
                },
                ingress_observation_surface: None,
            }));

            prop_assert_eq!(
                build_cursor_autocmd_events(
                    ingress,
                    observed_at,
                    needs_initialize,
                    buffer_perf_class,
                    ingress_cursor_presentation,
                    None,
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
            tracked_cursor_present in any::<bool>(),
        ) {
            let tracked_cursor = tracked_cursor_present
                .then(|| TrackedCursor::fixture(tracked_window_handle, 22, 3, 4));
            let snapshot = snapshot_with_state(
                true,
                Some(BufferPerfClass::Full),
                tracked_cursor,
            );
            let expected = ingress == AutocmdIngress::BufEnter
                && tracked_cursor_present
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
    fn conceal_probe_cache_invalidation_only_tracks_conceal_window_options() {
        for (option_name, expected) in [
            ("conceallevel", true),
            ("concealcursor", true),
            ("filetype", false),
            ("number", false),
        ] {
            assert_eq!(
                should_invalidate_conceal_probe_cache_for_option(option_name),
                expected,
                "unexpected conceal probe invalidation result for {option_name}"
            );
        }
    }

    #[test]
    fn text_mutation_autocmds_invalidate_metadata_without_requesting_observation() {
        for ingress in [
            AutocmdIngress::TextChanged,
            AutocmdIngress::TextChangedInsert,
        ] {
            assert!(!should_request_observation_for_autocmd(ingress));
            assert!(!ingress.supports_unchanged_fast_path());
        }
    }

    #[test]
    fn option_set_metadata_invalidation_drops_only_target_buffer_metadata_and_policy() {
        const TARGET_BUFFER_HANDLE: i64 = 11;
        const OTHER_BUFFER_HANDLE: i64 = 29;

        reset_buffer_local_cache_state();

        let target_metadata = BufferMetadata::new_for_test("lua", "", true, 42);
        let other_metadata = BufferMetadata::new_for_test("rust", "terminal", false, 99);
        let target_policy = BufferEventPolicy::from_buffer_metadata("", true, 42, 0.0);
        let other_policy = BufferEventPolicy::from_buffer_metadata("terminal", false, 99, 0.0);
        mutate_engine_state(|state| {
            state
                .shell
                .buffer_metadata_cache
                .store_for_test(TARGET_BUFFER_HANDLE, target_metadata.clone());
            state
                .shell
                .buffer_metadata_cache
                .store_for_test(OTHER_BUFFER_HANDLE, other_metadata.clone());
            state
                .shell
                .buffer_perf_policy_cache
                .store_policy(TARGET_BUFFER_HANDLE, target_policy);
            state
                .shell
                .buffer_perf_policy_cache
                .store_policy(OTHER_BUFFER_HANDLE, other_policy);
        })
        .expect("engine state access should succeed");

        invalidate_buffer_metadata(TARGET_BUFFER_HANDLE)
            .expect("metadata invalidation should succeed");

        let cached_entries = read_engine_state(|state| {
            (
                state
                    .shell
                    .buffer_metadata_cache
                    .cached_entry_for_test(TARGET_BUFFER_HANDLE),
                state
                    .shell
                    .buffer_metadata_cache
                    .cached_entry_for_test(OTHER_BUFFER_HANDLE),
                state
                    .shell
                    .buffer_perf_policy_cache
                    .cached_policy(TARGET_BUFFER_HANDLE),
                state
                    .shell
                    .buffer_perf_policy_cache
                    .cached_policy(OTHER_BUFFER_HANDLE),
            )
        })
        .expect("engine state access should succeed");

        assert_eq!(
            cached_entries,
            (None, Some(other_metadata), None, Some(other_policy))
        );
    }

    #[test]
    fn buffer_churn_invalidation_clears_all_target_buffer_local_caches() {
        const TARGET_BUFFER_HANDLE: i64 = 13;
        const OTHER_BUFFER_HANDLE: i64 = 31;

        reset_buffer_local_cache_state();

        let target_metadata = BufferMetadata::new_for_test("lua", "", true, 120);
        let other_metadata = BufferMetadata::new_for_test("rust", "terminal", false, 14);
        let target_policy = BufferEventPolicy::from_buffer_metadata("", true, 120, 0.0);
        let other_policy = BufferEventPolicy::from_buffer_metadata("terminal", false, 14, 0.0);
        let (target_telemetry, other_telemetry) = mutate_engine_state(|state| {
            state
                .shell
                .buffer_metadata_cache
                .store_for_test(TARGET_BUFFER_HANDLE, target_metadata.clone());
            state
                .shell
                .buffer_metadata_cache
                .store_for_test(OTHER_BUFFER_HANDLE, other_metadata.clone());
            state
                .shell
                .buffer_perf_policy_cache
                .store_policy(TARGET_BUFFER_HANDLE, target_policy);
            state
                .shell
                .buffer_perf_policy_cache
                .store_policy(OTHER_BUFFER_HANDLE, other_policy);
            (
                state
                    .shell
                    .buffer_perf_telemetry_cache
                    .record_conceal_full_scan(TARGET_BUFFER_HANDLE, 1_000.0),
                state
                    .shell
                    .buffer_perf_telemetry_cache
                    .record_cursor_color_extmark_fallback(OTHER_BUFFER_HANDLE, 1_500.0),
            )
        })
        .expect("engine state access should succeed");

        invalidate_buffer_local_caches(TARGET_BUFFER_HANDLE)
            .expect("buffer-local cache invalidation should succeed");

        let cached_entries = read_engine_state(|state| {
            (
                state
                    .shell
                    .buffer_metadata_cache
                    .cached_entry_for_test(TARGET_BUFFER_HANDLE),
                state
                    .shell
                    .buffer_metadata_cache
                    .cached_entry_for_test(OTHER_BUFFER_HANDLE),
                state
                    .shell
                    .buffer_perf_policy_cache
                    .cached_policy(TARGET_BUFFER_HANDLE),
                state
                    .shell
                    .buffer_perf_policy_cache
                    .cached_policy(OTHER_BUFFER_HANDLE),
                state
                    .shell
                    .buffer_perf_telemetry_cache
                    .telemetry(TARGET_BUFFER_HANDLE),
                state
                    .shell
                    .buffer_perf_telemetry_cache
                    .telemetry(OTHER_BUFFER_HANDLE),
            )
        })
        .expect("engine state access should succeed");

        assert_eq!(
            cached_entries,
            (
                None,
                Some(other_metadata),
                None,
                Some(other_policy),
                None,
                Some(other_telemetry),
            )
        );

        assert_eq!(target_telemetry.callback_duration_estimate_ms(), 0.0);
    }

    #[test]
    fn text_mutation_invalidation_clears_only_the_target_buffer_probe_entries() {
        const TARGET_BUFFER_HANDLE: i64 = 17;
        const OTHER_BUFFER_HANDLE: i64 = 37;

        reset_buffer_local_cache_state();

        let target_context_key =
            CursorTextContextCacheKey::new(TARGET_BUFFER_HANDLE, 14, 7, Some(6));
        let other_context_key = CursorTextContextCacheKey::new(OTHER_BUFFER_HANDLE, 14, 7, Some(6));
        let target_context = Some(CursorTextContext::new(
            TARGET_BUFFER_HANDLE,
            14,
            7,
            vec![],
            None,
        ));
        let other_context = Some(CursorTextContext::new(
            OTHER_BUFFER_HANDLE,
            14,
            7,
            vec![],
            None,
        ));
        let target_conceal_key = conceal_key(TARGET_BUFFER_HANDLE, 14, 7, 2, "n");
        let other_conceal_key = conceal_key(OTHER_BUFFER_HANDLE, 14, 7, 2, "n");
        let target_regions: Arc<[_]> = vec![conceal_region(3, 4, 11, 1)].into();
        let other_regions: Arc<[_]> = vec![conceal_region(8, 9, 12, 2)].into();

        mutate_engine_state(|state| {
            state
                .shell
                .probe_cache
                .store_cursor_text_context(target_context_key.clone(), target_context.clone());
            state
                .shell
                .probe_cache
                .store_cursor_text_context(other_context_key.clone(), other_context.clone());
            state.shell.probe_cache.store_conceal_regions(
                target_conceal_key.clone(),
                18,
                Arc::clone(&target_regions),
            );
            state.shell.probe_cache.store_conceal_regions(
                other_conceal_key.clone(),
                18,
                Arc::clone(&other_regions),
            );
        })
        .expect("engine state access should succeed");

        invalidate_buffer_local_probe_caches(TARGET_BUFFER_HANDLE)
            .expect("probe invalidation should succeed");

        let cached_entries = mutate_engine_state(|state| {
            (
                state
                    .shell
                    .probe_cache
                    .cached_cursor_text_context(&target_context_key),
                state
                    .shell
                    .probe_cache
                    .cached_cursor_text_context(&other_context_key),
                state
                    .shell
                    .probe_cache
                    .cached_conceal_regions(&target_conceal_key),
                state
                    .shell
                    .probe_cache
                    .cached_conceal_regions(&other_conceal_key),
            )
        })
        .expect("engine state access should succeed");

        assert_eq!(
            cached_entries,
            (
                CursorTextContextCacheLookup::Miss,
                CursorTextContextCacheLookup::Hit(other_context),
                ConcealCacheLookup::Miss,
                ConcealCacheLookup::Hit(CachedConcealRegions::new(18, other_regions)),
            )
        );
    }

    #[test]
    fn conceal_option_invalidation_clears_only_the_target_buffer_conceal_entries() {
        const TARGET_BUFFER_HANDLE: i64 = 19;
        const OTHER_BUFFER_HANDLE: i64 = 41;

        reset_buffer_local_cache_state();

        let target_context_key =
            CursorTextContextCacheKey::new(TARGET_BUFFER_HANDLE, 14, 7, Some(6));
        let target_context = Some(CursorTextContext::new(
            TARGET_BUFFER_HANDLE,
            14,
            7,
            vec![],
            None,
        ));
        let target_conceal_key = conceal_key(TARGET_BUFFER_HANDLE, 14, 7, 2, "n");
        let other_conceal_key = conceal_key(OTHER_BUFFER_HANDLE, 14, 7, 2, "n");
        let target_regions: Arc<[_]> = vec![conceal_region(3, 4, 11, 1)].into();
        let other_regions: Arc<[_]> = vec![conceal_region(8, 9, 12, 2)].into();

        mutate_engine_state(|state| {
            state
                .shell
                .probe_cache
                .store_cursor_text_context(target_context_key.clone(), target_context.clone());
            state.shell.probe_cache.store_conceal_regions(
                target_conceal_key.clone(),
                18,
                Arc::clone(&target_regions),
            );
            state.shell.probe_cache.store_conceal_regions(
                other_conceal_key.clone(),
                18,
                Arc::clone(&other_regions),
            );
        })
        .expect("engine state access should succeed");

        invalidate_conceal_probe_caches(TARGET_BUFFER_HANDLE)
            .expect("conceal option invalidation should succeed");

        let cached_entries = mutate_engine_state(|state| {
            (
                state
                    .shell
                    .probe_cache
                    .cached_cursor_text_context(&target_context_key),
                state
                    .shell
                    .probe_cache
                    .cached_conceal_regions(&target_conceal_key),
                state
                    .shell
                    .probe_cache
                    .cached_conceal_regions(&other_conceal_key),
            )
        })
        .expect("engine state access should succeed");

        assert_eq!(
            cached_entries,
            (
                CursorTextContextCacheLookup::Hit(target_context),
                ConcealCacheLookup::Miss,
                ConcealCacheLookup::Hit(CachedConcealRegions::new(18, other_regions)),
            )
        );
    }

    #[test]
    fn text_mutation_revision_advances_only_for_the_target_buffer() {
        const TARGET_BUFFER_HANDLE: i64 = 23;
        const OTHER_BUFFER_HANDLE: i64 = 47;

        reset_buffer_local_cache_state();

        mutate_engine_state(|state| {
            state
                .shell
                .buffer_text_revision_cache
                .advance(OTHER_BUFFER_HANDLE);
        })
        .expect("engine state access should succeed");

        advance_buffer_text_revision(TARGET_BUFFER_HANDLE)
            .expect("text revision advance should succeed");

        let revisions = mutate_engine_state(|state| {
            (
                state
                    .shell
                    .buffer_text_revision_cache
                    .cached_entry_for_test(TARGET_BUFFER_HANDLE),
                state
                    .shell
                    .buffer_text_revision_cache
                    .cached_entry_for_test(OTHER_BUFFER_HANDLE),
            )
        })
        .expect("engine state access should succeed");

        assert_eq!(
            revisions,
            (Some(Generation::new(1)), Some(Generation::new(1)))
        );
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
        let tracked_cursor = TrackedCursor::fixture(10, 20, 4, 12)
            .with_viewport_columns(3, 1)
            .with_window_origin(5, 7)
            .with_window_dimensions(80, 24);
        let matching_target = RenderPoint {
            row: 11.0,
            col: 22.0,
        };
        let matching_snapshot =
            fast_path_snapshot(true, false, Some(tracked_cursor.clone()), matching_target);

        for (label, ingress, snapshot, current_location, current_target, expected) in [
            (
                "cursor moved always stays live",
                AutocmdIngress::CursorMoved,
                matching_snapshot.clone(),
                Some(tracked_cursor.clone()),
                Some(matching_target),
                false,
            ),
            (
                "insert repeat always stays live",
                AutocmdIngress::CursorMovedInsert,
                matching_snapshot.clone(),
                Some(tracked_cursor.clone()),
                Some(matching_target),
                false,
            ),
            (
                "window scrolled repeat",
                AutocmdIngress::WinScrolled,
                matching_snapshot.clone(),
                Some(tracked_cursor.clone()),
                Some(matching_target),
                true,
            ),
            (
                "window enter repeat",
                AutocmdIngress::WinEnter,
                matching_snapshot.clone(),
                Some(tracked_cursor.clone()),
                Some(matching_target),
                true,
            ),
            (
                "buffer enter repeat",
                AutocmdIngress::BufEnter,
                matching_snapshot.clone(),
                Some(tracked_cursor.clone()),
                Some(matching_target),
                true,
            ),
            (
                "mode changes still require full path",
                AutocmdIngress::ModeChanged,
                matching_snapshot.clone(),
                Some(tracked_cursor.clone()),
                Some(matching_target),
                false,
            ),
            (
                "surface changes stay live",
                AutocmdIngress::CursorMoved,
                matching_snapshot.clone(),
                Some(TrackedCursor::fixture(10, 20, 4, 13)),
                Some(matching_target),
                false,
            ),
            (
                "target changes stay live",
                AutocmdIngress::CursorMoved,
                matching_snapshot.clone(),
                Some(tracked_cursor.clone()),
                Some(RenderPoint {
                    row: 12.0,
                    col: 22.0,
                }),
                false,
            ),
            (
                "missing live position disables the fast path",
                AutocmdIngress::CursorMoved,
                matching_snapshot,
                Some(tracked_cursor.clone()),
                None,
                false,
            ),
            (
                "uninitialized runtime disables the fast path",
                AutocmdIngress::CursorMoved,
                fast_path_snapshot(true, true, Some(tracked_cursor.clone()), matching_target),
                Some(tracked_cursor.clone()),
                Some(matching_target),
                false,
            ),
            (
                "disabled runtime disables the fast path",
                AutocmdIngress::CursorMoved,
                fast_path_snapshot(false, false, Some(tracked_cursor.clone()), matching_target),
                Some(tracked_cursor.clone()),
                Some(matching_target),
                false,
            ),
            (
                "missing tracked cursor disables the fast path",
                AutocmdIngress::CursorMoved,
                fast_path_snapshot(true, false, None, matching_target),
                Some(tracked_cursor),
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

    #[test]
    fn handle_only_fast_path_precheck_requires_matching_window_and_buffer_handles() {
        let tracked_cursor = TrackedCursor::fixture(10, 20, 4, 12);

        for (label, current_window_handle, current_buffer_handle, expected) in [
            ("matching handles", 10, 20, true),
            ("window drift", 11, 20, false),
            ("buffer drift", 10, 21, false),
            ("both drift", 11, 21, false),
        ] {
            assert_eq!(
                tracked_cursor_matches_live_surface_handles(
                    &tracked_cursor,
                    current_window_handle,
                    current_buffer_handle,
                ),
                expected,
                "{label}"
            );
        }
    }
}

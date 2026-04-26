use super::IngressDispatchOutcome;
use crate::core::effect::IngressCursorCommandLineLocation;
use crate::core::effect::IngressCursorModeAdmission;
use crate::core::effect::IngressCursorPresentationRequest;
use crate::core::effect::IngressObservationSurface;
use crate::core::effect::ProbePolicy;
use crate::core::event::Event as CoreEvent;
use crate::core::event::ExternalDemandQueuedEvent;
use crate::core::event::InitializeEvent;
use crate::core::state::BufferPerfClass;
use crate::core::state::ExternalDemandKind;
use crate::core::types::Millis;
use crate::events::cursor::cursor_observation_for_mode_with_probe_policy;
use crate::events::cursor::smear_outside_cmd_row;
use crate::events::handlers::core_dispatch::dispatch_core_events_with_default_scheduler;
use crate::events::handlers::source_selection::should_request_observation_for_autocmd;
use crate::events::handlers::viewport::surface_for_ingress_fast_path_with_current_editor;
use crate::events::ingress::AutocmdIngress;
use crate::events::logging::warn;
use crate::events::runtime::IngressReadSnapshot;
use crate::events::runtime::RuntimeAccessResult;
use crate::events::runtime::ingress_read_snapshot_with_current_buffer;
use crate::events::runtime::note_autocmd_event_now;
use crate::events::runtime::now_ms;
use crate::events::runtime::record_cursor_autocmd_fast_path_continued;
use crate::events::runtime::record_cursor_autocmd_fast_path_dropped;
use crate::events::runtime::to_core_millis;
use crate::events::runtime::with_core_read;
use crate::events::surface::current_window_surface_snapshot;
use crate::host::BufferHandle;
use crate::host::CurrentEditorPort;
use crate::host::NeovimHost;
use crate::host::api;
use crate::position::CursorObservation;
use crate::position::RenderPoint;
use crate::position::WindowSurfaceSnapshot;
use crate::state::TrackedCursor;
use crate::types::EPSILON;
use nvim_oxi::Result;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum CursorAutocmdFastPathOutcome {
    Dropped,
    Continue,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum CursorAutocmdPreflight {
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

#[derive(Debug, Clone, PartialEq)]
pub(super) struct CursorAutocmdFastPathSnapshot {
    pub(super) enabled: bool,
    pub(super) needs_initialize: bool,
    pub(super) tracked_cursor: Option<TrackedCursor>,
    pub(super) target_position: RenderPoint,
    pub(super) smear_to_cmd: bool,
}

pub(super) fn on_cursor_event_core_for_autocmd(
    ingress: AutocmdIngress,
) -> Result<IngressDispatchOutcome> {
    on_cursor_event_core_for_autocmd_with(&NeovimHost, ingress)
}

fn on_cursor_event_core_for_autocmd_with(
    host: &impl CurrentEditorPort,
    ingress: AutocmdIngress,
) -> Result<IngressDispatchOutcome> {
    let (current_surface, current_cursor, window, buffer) =
        match maybe_drop_unchanged_cursor_autocmd_with(host, ingress)? {
            CursorAutocmdFastPathResult::Dropped => return Ok(IngressDispatchOutcome::Dropped),
            CursorAutocmdFastPathResult::Continue {
                current_surface,
                current_cursor,
                window,
                buffer,
            } => (current_surface, current_cursor, window, buffer),
        };
    let window_valid = host.window_is_valid(&window);
    let buffer_valid = host.buffer_is_valid(&buffer);
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
    let mode = host.current_mode();
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
    let ingress_observation_surface = ingress_observation_surface(
        host,
        &window,
        &buffer,
        current_surface,
        current_cursor,
        mode,
    );
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

fn cursor_autocmd_fast_path_snapshot() -> RuntimeAccessResult<CursorAutocmdFastPathSnapshot> {
    with_core_read(|state| {
        let runtime = state.runtime();
        CursorAutocmdFastPathSnapshot {
            enabled: runtime.is_enabled(),
            needs_initialize: state.needs_initialize(),
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

pub(super) fn tracked_cursor_matches_live_surface_handles(
    tracked_cursor: &TrackedCursor,
    current_window_handle: i64,
    current_buffer_handle: impl Into<BufferHandle>,
) -> bool {
    let current_buffer_handle = current_buffer_handle.into();
    tracked_cursor.window_handle() == current_window_handle
        && tracked_cursor.buffer_handle() == current_buffer_handle
}

pub(super) fn should_drop_unchanged_cursor_autocmd(
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

pub(super) fn build_cursor_autocmd_events(
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

pub(super) fn demand_kind_for_autocmd(ingress: AutocmdIngress) -> ExternalDemandKind {
    match ingress {
        AutocmdIngress::ModeChanged => ExternalDemandKind::ModeChanged,
        AutocmdIngress::BufEnter => ExternalDemandKind::BufferEntered,
        AutocmdIngress::CmdlineChanged
        | AutocmdIngress::CursorMoved
        | AutocmdIngress::CursorMovedInsert
        | AutocmdIngress::WinEnter
        | AutocmdIngress::WinScrolled
        | AutocmdIngress::BufWipeout
        | AutocmdIngress::OptionSet
        | AutocmdIngress::TabClosed
        | AutocmdIngress::TextChanged
        | AutocmdIngress::TextChangedInsert
        | AutocmdIngress::VimResized
        | AutocmdIngress::WinClosed
        | AutocmdIngress::ColorScheme => ExternalDemandKind::ExternalCursor,
    }
}

fn collect_ingress_cursor_presentation_request(
    snapshot: &IngressReadSnapshot,
    mode: &str,
) -> Result<IngressCursorPresentationRequest> {
    let current_corners = snapshot.current_corners();
    let mode_admission = if snapshot.mode_allowed(mode) {
        IngressCursorModeAdmission::Allowed
    } else {
        IngressCursorModeAdmission::Blocked
    };
    let command_line_location = if smear_outside_cmd_row(&current_corners)? {
        IngressCursorCommandLineLocation::Outside
    } else {
        IngressCursorCommandLineLocation::Inside
    };

    Ok(IngressCursorPresentationRequest::new(
        mode_admission,
        command_line_location,
        snapshot.current_visual_cursor_cell(),
        snapshot.current_visual_cursor_shape(),
    ))
}

pub(super) fn should_coalesce_window_follow_up_autocmd(
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

pub(super) fn cursor_autocmd_preflight(
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

fn maybe_drop_unchanged_cursor_autocmd_with(
    host: &impl CurrentEditorPort,
    ingress: AutocmdIngress,
) -> Result<CursorAutocmdFastPathResult> {
    let window = host.current_window();
    let buffer = host.current_buffer();

    if !ingress.supports_unchanged_fast_path() {
        return Ok(continue_cursor_autocmd_fast_path(ingress, window, buffer));
    }

    let fast_path_snapshot = cursor_autocmd_fast_path_snapshot()?;
    if !fast_path_snapshot.enabled || fast_path_snapshot.needs_initialize {
        return Ok(continue_cursor_autocmd_fast_path(ingress, window, buffer));
    }

    let Some(tracked_cursor) = fast_path_snapshot.tracked_cursor.as_ref() else {
        return Ok(continue_cursor_autocmd_fast_path(ingress, window, buffer));
    };
    if !host.window_is_valid(&window)
        || i64::from(window.handle()) != tracked_cursor.window_handle()
    {
        return Ok(continue_cursor_autocmd_fast_path(ingress, window, buffer));
    }

    if !host.buffer_is_valid(&buffer)
        || !tracked_cursor_matches_live_surface_handles(
            tracked_cursor,
            i64::from(window.handle()),
            BufferHandle::from_buffer(&buffer),
        )
    {
        return Ok(continue_cursor_autocmd_fast_path(ingress, window, buffer));
    }

    let Some(current_surface) =
        surface_for_ingress_fast_path_with_current_editor(host, &window, &buffer)
    else {
        return Ok(continue_cursor_autocmd_fast_path(ingress, window, buffer));
    };
    let mode = host.current_mode();
    let current_cursor = current_cursor_observation_for_fast_path(
        &window,
        fast_path_snapshot.smear_to_cmd,
        &mode,
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
        return Ok(drop_cursor_autocmd_fast_path(ingress));
    }

    record_cursor_autocmd_fast_path_outcome(ingress, CursorAutocmdFastPathOutcome::Continue);
    Ok(CursorAutocmdFastPathResult::Continue {
        current_surface: Some(current_surface),
        current_cursor,
        window,
        buffer,
    })
}

fn continue_cursor_autocmd_fast_path(
    ingress: AutocmdIngress,
    window: api::Window,
    buffer: api::Buffer,
) -> CursorAutocmdFastPathResult {
    record_cursor_autocmd_fast_path_outcome(ingress, CursorAutocmdFastPathOutcome::Continue);
    CursorAutocmdFastPathResult::Continue {
        current_surface: None,
        current_cursor: None,
        window,
        buffer,
    }
}

fn drop_cursor_autocmd_fast_path(ingress: AutocmdIngress) -> CursorAutocmdFastPathResult {
    note_autocmd_event_now();
    record_cursor_autocmd_fast_path_outcome(ingress, CursorAutocmdFastPathOutcome::Dropped);
    CursorAutocmdFastPathResult::Dropped
}

fn ingress_observation_surface(
    host: &impl CurrentEditorPort,
    window: &api::Window,
    buffer: &api::Buffer,
    current_surface: Option<WindowSurfaceSnapshot>,
    current_cursor: Option<CursorObservation>,
    mode: String,
) -> Option<IngressObservationSurface> {
    if !host.window_is_valid(window) || !host.buffer_is_valid(buffer) {
        return None;
    }

    let surface = match current_surface {
        Some(surface) => surface,
        None => current_window_surface_snapshot(window)
            .ok()
            .filter(|surface| surface.id().buffer_handle() == BufferHandle::from_buffer(buffer))?,
    };

    Some(IngressObservationSurface::new(
        surface,
        current_cursor,
        mode,
    ))
}

#[cfg(test)]
mod tests {
    use super::CursorAutocmdFastPathResult;
    use super::maybe_drop_unchanged_cursor_autocmd_with;
    use crate::events::ingress::AutocmdIngress;
    use crate::events::runtime::reset_transient_event_state;
    use crate::host::CurrentEditorCall;
    use crate::host::FakeCurrentEditorPort;
    use pretty_assertions::assert_eq;

    #[test]
    fn cursor_autocmd_fast_path_reads_current_handles_through_current_editor_port() {
        reset_transient_event_state();
        let host = FakeCurrentEditorPort::default();
        host.set_current_window_handle(11);
        host.set_current_buffer_handle(17);

        let result = maybe_drop_unchanged_cursor_autocmd_with(&host, AutocmdIngress::CursorMoved)
            .expect("current handles should be readable");
        let CursorAutocmdFastPathResult::Continue { window, buffer, .. } = result else {
            panic!("unsupported unchanged fast-path ingress should continue");
        };

        assert_eq!((window.handle(), buffer.handle()), (11, 17));
        assert_eq!(
            host.calls(),
            vec![
                CurrentEditorCall::CurrentWindow,
                CurrentEditorCall::CurrentBuffer,
            ],
        );
        reset_transient_event_state();
    }
}

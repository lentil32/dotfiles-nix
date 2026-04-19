use super::cursor_color::current_cursor_color_probe_generations;
use super::cursor_color::mode_requires_cursor_color_sampling;
use super::text_context::current_cursor_text_context;
use crate::core::effect::CursorPositionReadPolicy;
use crate::core::effect::IngressObservationSurface;
use crate::core::effect::ProbePolicy;
use crate::core::effect::RequestObservationBaseEffect;
use crate::core::event::Event as CoreEvent;
use crate::core::event::ObservationBaseCollectedEvent;
use crate::core::state::CursorTextContextState;
use crate::core::state::ObservationBasis;
use crate::core::state::ObservationMotion;
use crate::core::types::Generation;
use crate::events::cursor::CursorReadError;
use crate::events::cursor::current_mode;
use crate::events::cursor::cursor_observation_for_mode_with_probe_policy_typed;
use crate::events::handlers::viewport::maybe_scroll_shift_for_core_event;
use crate::events::runtime::buffer_text_revision;
use crate::events::runtime::editor_viewport_for_bounds;
use crate::events::runtime::note_cursor_color_observation_boundary;
use crate::events::runtime::now_ms;
use crate::events::runtime::to_core_millis;
use crate::events::surface::WindowSurfaceReadError;
use crate::events::surface::current_window_surface_snapshot;
use crate::position::CursorObservation;
use crate::position::ObservedCell;
use crate::position::ViewportBounds;
use crate::position::WindowSurfaceSnapshot;
use nvim_oxi::Result;
use nvim_oxi::api;

pub(super) type ObservationReadResult<T> = std::result::Result<T, ObservationReadError>;

#[derive(Debug, thiserror::Error)]
pub(super) enum ObservationReadError {
    #[error(transparent)]
    Shell(#[from] nvim_oxi::Error),
    #[error(transparent)]
    Surface(#[from] WindowSurfaceReadError),
    #[error(transparent)]
    Cursor(#[from] CursorReadError),
    #[error("current window unavailable")]
    MissingWindow,
    #[error("current buffer unavailable")]
    MissingBuffer,
    #[error("current buffer text revision unavailable")]
    MissingBufferTextRevision,
    #[error("current viewport unavailable")]
    MissingViewport,
}

impl From<nvim_oxi::api::Error> for ObservationReadError {
    fn from(error: nvim_oxi::api::Error) -> Self {
        Self::Shell(error.into())
    }
}

impl From<ObservationReadError> for nvim_oxi::Error {
    fn from(error: ObservationReadError) -> Self {
        match error {
            ObservationReadError::Shell(error) => error,
            ObservationReadError::Surface(_)
            | ObservationReadError::Cursor(_)
            | ObservationReadError::MissingWindow
            | ObservationReadError::MissingBuffer
            | ObservationReadError::MissingBufferTextRevision
            | ObservationReadError::MissingViewport => {
                nvim_oxi::api::Error::Other(error.to_string()).into()
            }
        }
    }
}

pub(super) struct CurrentEditorSnapshot {
    mode: String,
    window: Option<api::Window>,
    buffer: Option<api::Buffer>,
    text_revision: Option<Generation>,
    viewport: Option<ViewportBounds>,
    ingress_surface: Option<WindowSurfaceSnapshot>,
    ingress_cursor: Option<CursorObservation>,
}

#[derive(Clone, Copy)]
enum ObservationCaptureSource<'a> {
    Live,
    Ingress(&'a IngressObservationSurface),
}

impl<'a> ObservationCaptureSource<'a> {
    const fn from_ingress(
        ingress_observation_surface: Option<&'a IngressObservationSurface>,
    ) -> Self {
        match ingress_observation_surface {
            Some(surface) => Self::Ingress(surface),
            None => Self::Live,
        }
    }

    fn map_or_else<T>(
        self,
        ingress: impl FnOnce(&IngressObservationSurface) -> T,
        live: impl FnOnce() -> T,
    ) -> T {
        match self {
            Self::Live => live(),
            Self::Ingress(surface) => ingress(surface),
        }
    }
}

impl CurrentEditorSnapshot {
    pub(super) fn capture() -> ObservationReadResult<Self> {
        Self::capture_with_viewport(false, None)
    }

    pub(super) fn capture_for_observation_base(
        ingress_observation_surface: Option<&IngressObservationSurface>,
    ) -> ObservationReadResult<Self> {
        Self::capture_with_viewport(true, ingress_observation_surface)
    }

    fn capture_with_viewport(
        include_viewport: bool,
        ingress_observation_surface: Option<&IngressObservationSurface>,
    ) -> ObservationReadResult<Self> {
        let source = ObservationCaptureSource::from_ingress(ingress_observation_surface);
        let mode = source.map_or_else(
            |surface| surface.mode().to_owned(),
            || current_mode().to_string_lossy().into_owned(),
        );
        let viewport = include_viewport
            .then(current_viewport_snapshot)
            .transpose()?;
        let window = source.map_or_else(window_from_ingress_surface, current_window_snapshot);
        let buffer = source.map_or_else(buffer_from_ingress_surface, current_buffer_snapshot);
        let text_revision = match buffer.as_ref() {
            Some(buffer) => Some(
                buffer_text_revision(i64::from(buffer.handle())).map_err(nvim_oxi::Error::from)?,
            ),
            None => None,
        };

        Ok(Self {
            mode,
            window,
            buffer,
            text_revision,
            viewport,
            ingress_surface: ingress_observation_surface.map(IngressObservationSurface::surface),
            ingress_cursor: ingress_observation_surface.and_then(IngressObservationSurface::cursor),
        })
    }

    pub(super) fn mode(&self) -> &str {
        self.mode.as_str()
    }

    pub(super) fn window(&self) -> Option<&api::Window> {
        self.window.as_ref()
    }

    pub(super) fn current_window(&self) -> ObservationReadResult<&api::Window> {
        self.window().ok_or(ObservationReadError::MissingWindow)
    }

    pub(super) fn buffer(&self) -> Option<&api::Buffer> {
        self.buffer.as_ref()
    }

    pub(super) fn current_buffer(&self) -> ObservationReadResult<&api::Buffer> {
        self.buffer().ok_or(ObservationReadError::MissingBuffer)
    }

    pub(super) fn text_revision(&self) -> Option<Generation> {
        self.text_revision
    }

    pub(super) fn current_text_revision(&self) -> ObservationReadResult<Generation> {
        self.text_revision()
            .ok_or(ObservationReadError::MissingBufferTextRevision)
    }

    pub(super) fn viewport(&self) -> Option<ViewportBounds> {
        self.viewport
    }

    pub(super) fn current_viewport(&self) -> ObservationReadResult<ViewportBounds> {
        self.viewport().ok_or(ObservationReadError::MissingViewport)
    }

    fn ingress_surface(&self) -> Option<&WindowSurfaceSnapshot> {
        self.ingress_surface.as_ref()
    }

    fn ingress_cursor(&self) -> Option<CursorObservation> {
        self.ingress_cursor
    }
}

fn current_window_snapshot() -> Option<api::Window> {
    let window = api::get_current_win();
    window.is_valid().then_some(window)
}

fn current_buffer_snapshot() -> Option<api::Buffer> {
    let buffer = api::get_current_buf();
    buffer.is_valid().then_some(buffer)
}

fn window_from_ingress_surface(surface: &IngressObservationSurface) -> Option<api::Window> {
    let handle = i32::try_from(surface.window_handle()).ok()?;
    let window = api::Window::from(handle);
    window.is_valid().then_some(window)
}

fn buffer_from_ingress_surface(surface: &IngressObservationSurface) -> Option<api::Buffer> {
    let handle = i32::try_from(surface.buffer_handle()).ok()?;
    let buffer = api::Buffer::from(handle);
    buffer.is_valid().then_some(buffer)
}

pub(super) fn current_core_cursor_position(
    snapshot: &CurrentEditorSnapshot,
    policy: CursorPositionReadPolicy,
    probe_policy: ProbePolicy,
) -> Result<ObservedCell> {
    if snapshot.window().is_none() {
        return Ok(ObservedCell::Unavailable);
    }
    let surface = current_observation_surface(snapshot).map_err(nvim_oxi::Error::from)?;
    current_core_cursor_observation(snapshot, policy, probe_policy, Some(&surface))
        .map(CursorObservation::cell)
        .map_err(nvim_oxi::Error::from)
}

pub(super) fn current_viewport_snapshot() -> ObservationReadResult<ViewportBounds> {
    let viewport = editor_viewport_for_bounds()?;
    viewport
        .bounds()
        .ok_or(ObservationReadError::MissingViewport)
}

fn observation_surface_from_sources(
    ingress_surface: Option<&WindowSurfaceSnapshot>,
    live_surface: impl FnOnce() -> ObservationReadResult<WindowSurfaceSnapshot>,
) -> ObservationReadResult<WindowSurfaceSnapshot> {
    ingress_surface.copied().map_or_else(live_surface, Ok)
}

fn current_observation_surface(
    snapshot: &CurrentEditorSnapshot,
) -> ObservationReadResult<WindowSurfaceSnapshot> {
    observation_surface_from_sources(snapshot.ingress_surface(), || {
        current_window_surface_snapshot(snapshot.current_window()?).map_err(Into::into)
    })
}

fn cursor_observation_from_sources(
    ingress_cursor: Option<CursorObservation>,
    live_cursor: impl FnOnce() -> ObservationReadResult<CursorObservation>,
) -> ObservationReadResult<CursorObservation> {
    ingress_cursor.map_or_else(live_cursor, Ok)
}

fn current_core_cursor_observation(
    snapshot: &CurrentEditorSnapshot,
    policy: CursorPositionReadPolicy,
    probe_policy: ProbePolicy,
    surface: Option<&WindowSurfaceSnapshot>,
) -> ObservationReadResult<CursorObservation> {
    cursor_observation_from_sources(snapshot.ingress_cursor(), || {
        cursor_observation_for_mode_with_probe_policy_typed(
            snapshot.current_window()?,
            snapshot.mode(),
            policy.smear_to_cmd(),
            probe_policy,
            surface,
        )
        .map_err(Into::into)
    })
}

fn collect_observation_basis(
    payload: &RequestObservationBaseEffect,
) -> ObservationReadResult<(
    ObservationBasis,
    Option<crate::core::state::CursorColorProbeGenerations>,
    ObservationMotion,
)> {
    let observed_at = to_core_millis(now_ms());
    let editor = CurrentEditorSnapshot::capture_for_observation_base(
        payload.context.ingress_observation_surface(),
    )?;
    let mode = editor.mode();
    let tracked_buffer_position = payload.context.tracked_buffer_position();
    let viewport = editor.current_viewport()?;
    let surface = current_observation_surface(&editor)?;
    let cursor = current_core_cursor_observation(
        &editor,
        payload.context.cursor_position_policy(),
        payload.context.probe_policy(),
        Some(&surface),
    )?;
    let buffer_revision = editor.text_revision().map(Generation::value);
    let current_cursor_text_context = match current_cursor_text_context(
        &editor,
        cursor.buffer_line().value(),
        tracked_buffer_position,
        payload.context.cursor_text_context_boundary(),
    ) {
        Ok(state) => state,
        Err(err) => {
            crate::events::logging::warn(&format!("cursor text context read failed: {err}"));
            CursorTextContextState::Unavailable
        }
    };
    let cursor_color_probe_generations = if payload.request.requested_probes().cursor_color()
        && mode_requires_cursor_color_sampling(mode)?
    {
        if let Err(err) = note_cursor_color_observation_boundary() {
            crate::events::logging::warn(&format!(
                "cursor color cache boundary update failed: {err}"
            ));
        }
        match current_cursor_color_probe_generations() {
            Ok(generations) if buffer_revision.is_some() => Some(generations),
            Ok(_) => {
                crate::events::logging::warn(
                    "cursor color probe generations missing buffer revision",
                );
                None
            }
            Err(err) => {
                crate::events::logging::warn(&format!(
                    "cursor color probe generations read failed: {err}"
                ));
                None
            }
        }
    } else {
        None
    };
    let basis = ObservationBasis::new(observed_at, mode.to_owned(), surface, cursor, viewport)
        .with_buffer_revision(buffer_revision)
        .with_cursor_text_context_state(current_cursor_text_context);
    let scroll_shift = match editor.window() {
        Some(window) => maybe_scroll_shift_for_core_event(window, &payload.context, &surface)?,
        None => None,
    };

    Ok((
        basis,
        cursor_color_probe_generations,
        ObservationMotion::new(scroll_shift),
    ))
}

pub(crate) fn execute_core_request_observation_base_effect(
    payload: RequestObservationBaseEffect,
) -> Result<Vec<CoreEvent>> {
    let (basis, cursor_color_probe_generations, motion) =
        collect_observation_basis(&payload).map_err(nvim_oxi::Error::from)?;
    Ok(vec![CoreEvent::ObservationBaseCollected(
        ObservationBaseCollectedEvent {
            observation_id: payload.request.observation_id(),
            basis,
            cursor_color_probe_generations,
            motion,
        },
    )])
}

#[cfg(test)]
mod tests {
    use super::CurrentEditorSnapshot;
    use super::ObservationCaptureSource;
    use super::ObservationReadError;
    use super::current_viewport_snapshot;
    use super::cursor_observation_from_sources;
    use super::observation_surface_from_sources;
    use crate::core::effect::IngressObservationSurface;
    use crate::events::cursor::CursorParseError;
    use crate::events::cursor::CursorReadError;
    use crate::events::surface::WindowSurfaceReadError;
    use crate::position::BufferLine;
    use crate::position::CursorObservation;
    use crate::position::ObservedCell;
    use crate::position::ScreenCell;
    use crate::position::SurfaceId;
    use crate::position::ViewportBounds;
    use crate::position::WindowSurfaceSnapshot;
    use pretty_assertions::assert_eq;

    fn surface_snapshot() -> WindowSurfaceSnapshot {
        WindowSurfaceSnapshot::new(
            SurfaceId::new(11, 17).expect("positive surface handles"),
            BufferLine::new(23).expect("positive buffer line"),
            5,
            2,
            ScreenCell::new(7, 13).expect("one-based origin"),
            ViewportBounds::new(24, 80).expect("positive viewport"),
        )
    }

    fn cursor_observation() -> CursorObservation {
        CursorObservation::new(
            BufferLine::new(23).expect("positive buffer line"),
            ObservedCell::Deferred(ScreenCell::new(7, 19).expect("one-based cursor cell")),
        )
    }

    fn ingress_surface() -> IngressObservationSurface {
        IngressObservationSurface::new(surface_snapshot(), Some(cursor_observation()), "n".into())
    }

    #[test]
    fn observation_capture_source_prefers_ingress_values_without_live_fallback() {
        let ingress_surface = ingress_surface();
        let source = ObservationCaptureSource::from_ingress(Some(&ingress_surface));

        assert_eq!(
            source.map_or_else(
                |surface| surface.mode().to_owned(),
                || panic!("ingress-backed capture should not consult the live fallback"),
            ),
            "n".to_string(),
        );
    }

    #[test]
    fn observation_capture_source_uses_live_values_without_ingress_surface() {
        let source = ObservationCaptureSource::from_ingress(None);

        assert_eq!(
            source.map_or_else(
                |_| panic!("live capture should not use ingress-only values"),
                || "n".to_string(),
            ),
            "n".to_string(),
        );
    }

    #[test]
    fn required_editor_snapshot_facts_return_typed_unavailable_errors() {
        let snapshot = CurrentEditorSnapshot {
            mode: "n".to_string(),
            window: None,
            buffer: None,
            text_revision: None,
            viewport: None,
            ingress_surface: None,
            ingress_cursor: None,
        };

        assert!(matches!(
            snapshot.current_window(),
            Err(ObservationReadError::MissingWindow)
        ));
        assert!(matches!(
            snapshot.current_buffer(),
            Err(ObservationReadError::MissingBuffer)
        ));
        assert!(matches!(
            snapshot.current_text_revision(),
            Err(ObservationReadError::MissingBufferTextRevision)
        ));
        assert!(matches!(
            snapshot.current_viewport(),
            Err(ObservationReadError::MissingViewport)
        ));
    }

    #[test]
    fn current_viewport_snapshot_uses_editor_viewport_owner_for_command_row_math() {
        crate::events::runtime::reset_transient_event_state();
        let viewport = crate::events::runtime::EditorViewportSnapshot::from_dimensions(40, 2, 120);
        crate::events::runtime::mutate_engine_state(|state| {
            state.shell.editor_viewport_cache.store_for_test(viewport);
        })
        .expect("engine state access should succeed");

        assert_eq!(
            current_viewport_snapshot().expect("cached viewport should yield observation bounds"),
            ViewportBounds::new(39, 120).expect("command row math should produce stable bounds")
        );

        crate::events::runtime::reset_transient_event_state();
    }

    #[test]
    fn observation_surface_uses_ingress_snapshot_without_touching_the_live_reader() {
        let surface = surface_snapshot();

        assert_eq!(
            observation_surface_from_sources(Some(&surface), || {
                panic!("ingress snapshot should bypass the live surface reader")
            })
            .expect("ingress surface should win"),
            surface,
        );
    }

    #[test]
    fn malformed_surface_payloads_fail_without_fabricating_a_zero_surface_snapshot() {
        let error = observation_surface_from_sources(None, || {
            Err(WindowSurfaceReadError::InvalidTopline { topline: 0 }.into())
        })
        .expect_err("malformed surface payload should fail");

        assert!(matches!(
            error,
            ObservationReadError::Surface(WindowSurfaceReadError::InvalidTopline { topline: 0 })
        ));
    }

    #[test]
    fn observation_cursor_uses_ingress_snapshot_without_touching_the_live_reader() {
        let cursor = cursor_observation();

        assert_eq!(
            cursor_observation_from_sources(Some(cursor), || {
                panic!("ingress cursor should bypass the live cursor reader")
            })
            .expect("ingress cursor should win"),
            cursor,
        );
    }

    #[test]
    fn malformed_cursor_payloads_fail_without_fabricating_unavailable_cursor_state() {
        let error = cursor_observation_from_sources(None, || {
            Err(ObservationReadError::Cursor(CursorReadError::Parse(
                CursorParseError::ScreenposInvalidCell { row: 0, col: 3 },
            )))
        })
        .expect_err("malformed cursor payload should fail");

        assert!(matches!(
            error,
            ObservationReadError::Cursor(CursorReadError::Parse(
                CursorParseError::ScreenposInvalidCell { row: 0, col: 3 }
            ))
        ));
    }
}

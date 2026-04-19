use super::cursor_color::current_cursor_color_probe_generations;
use super::cursor_color::mode_requires_cursor_color_sampling;
use super::text_context::current_cursor_text_context;
use crate::core::effect::CursorPositionReadPolicy;
use crate::core::effect::IngressObservationSurface;
use crate::core::effect::ProbePolicy;
use crate::core::effect::RequestObservationBaseEffect;
use crate::core::event::Event as CoreEvent;
use crate::core::event::ObservationBaseCollectedEvent;
use crate::core::state::CursorPositionSync;
use crate::core::state::CursorTextContextState;
use crate::core::state::ObservationBasis;
use crate::core::state::ObservationMotion;
use crate::core::types::CursorCol;
use crate::core::types::CursorPosition;
use crate::core::types::CursorRow;
use crate::core::types::Generation;
use crate::core::types::ViewportSnapshot;
use crate::draw::editor_bounds;
use crate::events::cursor::current_mode;
use crate::events::cursor::cursor_position_read_for_mode_with_probe_policy;
use crate::events::handlers::viewport::cursor_location_for_core_render;
use crate::events::handlers::viewport::maybe_scroll_shift_for_core_event;
use crate::events::runtime::buffer_text_revision;
use crate::events::runtime::note_cursor_color_observation_boundary;
use crate::events::runtime::now_ms;
use crate::events::runtime::to_core_millis;
use crate::state::CursorLocation;
use nvim_oxi::Result;
use nvim_oxi::api;

fn to_core_coordinate(value: f64) -> Option<u32> {
    if !value.is_finite() || value < 0.0 || value > u32::MAX as f64 {
        return None;
    }
    Some(value as u32)
}

fn to_core_dimension(value: i64) -> Option<u32> {
    if value < 1 || value > i64::from(u32::MAX) {
        return None;
    }
    u32::try_from(value).ok()
}

pub(super) struct CurrentCoreCursorPosition {
    pub(super) position: Option<CursorPosition>,
    pub(super) sync: CursorPositionSync,
}

pub(super) struct CurrentEditorSnapshot {
    mode: String,
    window: Option<api::Window>,
    buffer: Option<api::Buffer>,
    text_revision: Option<Generation>,
    viewport: Option<ViewportSnapshot>,
    ingress_cursor_location: Option<CursorLocation>,
}

impl CurrentEditorSnapshot {
    pub(super) fn capture() -> Result<Self> {
        Self::capture_with_viewport(false, None)
    }

    pub(super) fn capture_for_observation_base(
        ingress_observation_surface: Option<&IngressObservationSurface>,
    ) -> Result<Self> {
        Self::capture_with_viewport(true, ingress_observation_surface)
    }

    fn capture_with_viewport(
        include_viewport: bool,
        ingress_observation_surface: Option<&IngressObservationSurface>,
    ) -> Result<Self> {
        let mode = ingress_observation_surface
            .map(IngressObservationSurface::mode)
            .map(str::to_owned)
            .unwrap_or_else(|| current_mode().to_string_lossy().into_owned());
        let viewport = include_viewport
            .then(current_viewport_snapshot)
            .transpose()?;
        let window = ingress_observation_surface
            .and_then(window_from_ingress_surface)
            .or_else(current_window_snapshot);
        let buffer = ingress_observation_surface
            .and_then(buffer_from_ingress_surface)
            .or_else(current_buffer_snapshot);
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
            ingress_cursor_location: ingress_observation_surface
                .and_then(IngressObservationSurface::cursor_location),
        })
    }

    pub(super) fn mode(&self) -> &str {
        self.mode.as_str()
    }

    pub(super) fn window(&self) -> Option<&api::Window> {
        self.window.as_ref()
    }

    pub(super) fn current_window(&self) -> Result<&api::Window> {
        self.window()
            .ok_or_else(|| nvim_oxi::api::Error::Other("current window invalid".into()).into())
    }

    pub(super) fn buffer(&self) -> Option<&api::Buffer> {
        self.buffer.as_ref()
    }

    pub(super) fn current_buffer(&self) -> Result<&api::Buffer> {
        self.buffer()
            .ok_or_else(|| nvim_oxi::api::Error::Other("current buffer invalid".into()).into())
    }

    pub(super) fn text_revision(&self) -> Option<Generation> {
        self.text_revision
    }

    pub(super) fn current_text_revision(&self) -> Result<Generation> {
        self.text_revision().ok_or_else(|| {
            nvim_oxi::api::Error::Other("current buffer text revision unavailable".into()).into()
        })
    }

    pub(super) fn viewport(&self) -> Option<ViewportSnapshot> {
        self.viewport
    }

    pub(super) fn current_viewport(&self) -> Result<ViewportSnapshot> {
        self.viewport().ok_or_else(|| {
            nvim_oxi::api::Error::Other("current viewport unavailable".into()).into()
        })
    }

    fn ingress_cursor_location(&self) -> Option<CursorLocation> {
        self.ingress_cursor_location.clone()
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
) -> Result<CurrentCoreCursorPosition> {
    let Some(window) = snapshot.window() else {
        return Ok(CurrentCoreCursorPosition {
            position: None,
            sync: CursorPositionSync::Exact,
        });
    };
    let mode = snapshot.mode();

    let cursor_read = cursor_position_read_for_mode_with_probe_policy(
        window,
        mode,
        policy.smear_to_cmd(),
        probe_policy,
    )?;
    let Some((row, col)) = cursor_read.position() else {
        return Ok(CurrentCoreCursorPosition {
            position: None,
            sync: cursor_read.sync(),
        });
    };

    let Some(row) = to_core_coordinate(row) else {
        return Ok(CurrentCoreCursorPosition {
            position: None,
            sync: cursor_read.sync(),
        });
    };
    let Some(col) = to_core_coordinate(col) else {
        return Ok(CurrentCoreCursorPosition {
            position: None,
            sync: cursor_read.sync(),
        });
    };

    Ok(CurrentCoreCursorPosition {
        position: Some(CursorPosition {
            row: CursorRow(row),
            col: CursorCol(col),
        }),
        sync: cursor_read.sync(),
    })
}

pub(super) fn current_viewport_snapshot() -> Result<ViewportSnapshot> {
    let viewport = editor_bounds()?;
    Ok(ViewportSnapshot::new(
        CursorRow(to_core_dimension(viewport.max_row).unwrap_or(1)),
        CursorCol(to_core_dimension(viewport.max_col).unwrap_or(1)),
    ))
}

fn collect_observation_basis(
    payload: &RequestObservationBaseEffect,
) -> Result<(
    ObservationBasis,
    Option<crate::core::state::CursorColorProbeGenerations>,
    ObservationMotion,
)> {
    let observed_at = to_core_millis(now_ms());
    let editor = CurrentEditorSnapshot::capture_for_observation_base(
        payload.context.ingress_observation_surface(),
    )?;
    let mode = editor.mode();
    let tracked_location = payload.context.tracked_location();
    let cursor_location = cursor_location_for_core_render(
        editor.window(),
        editor.buffer(),
        tracked_location.clone(),
        editor.ingress_cursor_location(),
    );
    let viewport = editor.current_viewport()?;
    let cursor_position = current_core_cursor_position(
        &editor,
        payload.context.cursor_position_policy(),
        payload.context.probe_policy(),
    )?;
    let buffer_revision = editor.text_revision().map(Generation::value);
    let current_cursor_text_context = match current_cursor_text_context(
        &editor,
        cursor_location.line,
        tracked_location.as_ref(),
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
    let scroll_shift = match editor.window() {
        Some(window) => {
            maybe_scroll_shift_for_core_event(window, &payload.context, &cursor_location)?
        }
        None => None,
    };

    Ok((
        ObservationBasis::new(
            observed_at,
            mode.to_owned(),
            cursor_position.position,
            cursor_location,
            viewport,
        )
        .with_buffer_revision(buffer_revision)
        .with_cursor_text_context_state(current_cursor_text_context),
        cursor_color_probe_generations,
        ObservationMotion::new(scroll_shift).with_cursor_position_sync(cursor_position.sync),
    ))
}

pub(crate) fn execute_core_request_observation_base_effect(
    payload: RequestObservationBaseEffect,
) -> Result<Vec<CoreEvent>> {
    let (basis, cursor_color_probe_generations, motion) = collect_observation_basis(&payload)?;
    Ok(vec![CoreEvent::ObservationBaseCollected(
        ObservationBaseCollectedEvent {
            observation_id: payload.request.observation_id(),
            basis,
            cursor_color_probe_generations,
            motion,
        },
    )])
}

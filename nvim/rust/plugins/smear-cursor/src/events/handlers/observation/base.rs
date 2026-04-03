use super::cursor_color::current_cursor_color_probe_witness;
use super::cursor_color::mode_requires_cursor_color_sampling;
use super::text_context::current_cursor_text_context;
use crate::core::effect::CursorPositionReadPolicy;
use crate::core::effect::ProbePolicy;
use crate::core::effect::RequestObservationBaseEffect;
use crate::core::event::Event as CoreEvent;
use crate::core::event::ObservationBaseCollectedEvent;
use crate::core::state::CursorPositionSync;
use crate::core::state::ObservationBasis;
use crate::core::state::ObservationMotion;
use crate::core::types::CursorCol;
use crate::core::types::CursorPosition;
use crate::core::types::CursorRow;
use crate::core::types::ViewportSnapshot;
use crate::draw::editor_bounds;
use crate::events::cursor::cursor_position_read_for_mode_with_probe_policy;
use crate::events::cursor::mode_string;
use crate::events::handlers::viewport::cursor_location_for_core_render;
use crate::events::handlers::viewport::maybe_scroll_shift_for_core_event;
use crate::events::runtime::now_ms;
use crate::events::runtime::to_core_millis;
use crate::lua::i64_from_object;
use nvim_oxi::Array;
use nvim_oxi::Object;
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

pub(super) fn current_core_cursor_position(
    mode: &str,
    policy: CursorPositionReadPolicy,
    probe_policy: ProbePolicy,
) -> Result<CurrentCoreCursorPosition> {
    let window = api::get_current_win();
    if !window.is_valid() {
        return Ok(CurrentCoreCursorPosition {
            position: None,
            sync: CursorPositionSync::Exact,
        });
    }

    let cursor_read = cursor_position_read_for_mode_with_probe_policy(
        &window,
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

pub(super) fn current_buffer_changedtick(buffer_handle: i64) -> Result<u64> {
    let args = Array::from_iter([Object::from(buffer_handle), Object::from("changedtick")]);
    let value = api::call_function("getbufvar", args)?;
    let changedtick = i64_from_object("getbufvar(changedtick)", value)?;
    if changedtick < 0 {
        return Err(nvim_oxi::api::Error::Other(
            "cursor color changedtick must be non-negative".into(),
        )
        .into());
    }

    Ok(changedtick as u64)
}

fn collect_observation_basis(
    payload: &RequestObservationBaseEffect,
) -> Result<(ObservationBasis, ObservationMotion)> {
    let observed_at = to_core_millis(now_ms());
    let mode = mode_string();
    let tracked_location = payload.context.tracked_location();
    let cursor_location = cursor_location_for_core_render(tracked_location.clone());
    let viewport = current_viewport_snapshot()?;
    let cursor_position = current_core_cursor_position(
        &mode,
        payload.context.cursor_position_policy(),
        payload.context.probe_policy(),
    )?;
    let cursor_text_context =
        match current_cursor_text_context(cursor_location.line, tracked_location.as_ref()) {
            Ok(context) => context,
            Err(err) => {
                crate::events::logging::warn(&format!("cursor text context read failed: {err}"));
                None
            }
        };
    let cursor_color_witness = if payload.request.probes().cursor_color()
        && mode_requires_cursor_color_sampling(&mode)?
    {
        match current_cursor_color_probe_witness(&mode, cursor_position.position) {
            Ok(witness) => Some(witness),
            Err(err) => {
                crate::events::logging::warn(&format!("cursor color witness read failed: {err}"));
                None
            }
        }
    } else {
        None
    };
    let scroll_shift = {
        let window = api::get_current_win();
        if !window.is_valid() {
            None
        } else {
            maybe_scroll_shift_for_core_event(&window, &payload.context, &cursor_location)?
        }
    };

    Ok((
        ObservationBasis::new(
            payload.request.observation_id(),
            observed_at,
            mode,
            cursor_position.position,
            cursor_location,
            viewport,
        )
        .with_cursor_color_witness(cursor_color_witness)
        .with_cursor_text_context(cursor_text_context),
        ObservationMotion::new(scroll_shift).with_cursor_position_sync(cursor_position.sync),
    ))
}

pub(crate) fn execute_core_request_observation_base_effect(
    payload: RequestObservationBaseEffect,
) -> Result<Vec<CoreEvent>> {
    let (basis, motion) = collect_observation_basis(&payload)?;
    Ok(vec![CoreEvent::ObservationBaseCollected(
        ObservationBaseCollectedEvent {
            request: payload.request,
            basis,
            motion,
        },
    )])
}

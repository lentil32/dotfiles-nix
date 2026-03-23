use super::super::cursor::{
    cursor_position_for_mode, mode_string, sampled_cursor_color_at_current_position,
};
use super::super::host_bridge::installed_host_bridge;
use super::super::logging::warn;
use super::super::probe_cache::CursorColorCacheLookup;
use super::super::runtime::{
    cached_cursor_color_sample, cursor_color_colorscheme_generation, now_ms,
    store_cursor_color_sample, to_core_millis,
};
use super::viewport::{cursor_location_for_core_render, maybe_scroll_shift_for_core_event};
use crate::core::effect::{
    CursorPositionReadPolicy, RequestObservationBaseEffect, RequestProbeEffect,
};
use crate::core::event::{Event as CoreEvent, ObservationBaseCollectedEvent, ProbeReportedEvent};
use crate::core::state::{
    BackgroundProbeBatch, BackgroundProbeChunk, CursorColorProbeWitness, CursorColorSample,
    CursorTextContext, ObservationBasis, ObservationMotion, ObservedTextRow, ProbeFailure,
    ProbeKind, ProbeReuse,
};
use crate::core::types::{CursorCol, CursorPosition, CursorRow, ViewportSnapshot};
use crate::draw::{
    BRAILLE_CODE_MAX, BRAILLE_CODE_MIN, OCTANT_CODE_MAX, OCTANT_CODE_MIN, editor_bounds,
};
use crate::lua::{
    LuaParseError, bool_from_object_typed, i64_from_object, parse_indexed_objects_typed,
};
use nvim_oxi::{Array, Object, Result, api};
use thiserror::Error;

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

fn current_core_cursor_position(
    mode: &str,
    policy: CursorPositionReadPolicy,
) -> Result<Option<CursorPosition>> {
    let window = api::get_current_win();
    if !window.is_valid() {
        return Ok(None);
    }

    let Some((row, col)) = cursor_position_for_mode(&window, mode, policy.smear_to_cmd())? else {
        return Ok(None);
    };

    let Some(row) = to_core_coordinate(row) else {
        return Ok(None);
    };
    let Some(col) = to_core_coordinate(col) else {
        return Ok(None);
    };

    Ok(Some(CursorPosition {
        row: CursorRow(row),
        col: CursorCol(col),
    }))
}

fn current_viewport_snapshot() -> Result<ViewportSnapshot> {
    let viewport = editor_bounds()?;
    Ok(ViewportSnapshot::new(
        CursorRow(to_core_dimension(viewport.max_row).unwrap_or(1)),
        CursorCol(to_core_dimension(viewport.max_col).unwrap_or(1)),
    ))
}

fn current_buffer_changedtick(buffer_handle: i64) -> Result<u64> {
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

fn observed_text_rows(buffer: &api::Buffer, center_line: i64) -> Result<Vec<ObservedTextRow>> {
    if center_line < 1 {
        return Ok(Vec::new());
    }

    let start_line = center_line.saturating_sub(1).max(1);
    let end_line = center_line.saturating_add(1);
    let start_index = usize::try_from(start_line.saturating_sub(1)).ok();
    let end_index = usize::try_from(end_line).ok();
    let (Some(start_index), Some(end_index)) = (start_index, end_index) else {
        return Ok(Vec::new());
    };

    buffer
        .get_lines(start_index..end_index, false)?
        .enumerate()
        .map(|(offset, line)| {
            let relative_line = match i64::try_from(offset) {
                Ok(offset_line) => start_line.saturating_add(offset_line),
                Err(_) => start_line,
            };
            Ok(ObservedTextRow::new(
                relative_line,
                line.to_string_lossy().into_owned(),
            ))
        })
        .collect()
}

fn current_cursor_text_context(
    cursor_line: i64,
    tracked_location: Option<&crate::state::CursorLocation>,
) -> Result<Option<CursorTextContext>> {
    if cursor_line < 1 {
        return Ok(None);
    }

    let buffer = api::get_current_buf();
    if !buffer.is_valid() {
        return Ok(None);
    }

    let buffer_handle = i64::from(buffer.handle());
    let changedtick = current_buffer_changedtick(buffer_handle)?;
    // Surprising: embedded Neovim does not expose Neovide's redraw grid here, so semantic
    // mutation detection uses narrow buffer-line snapshots plus changedtick instead of UI cells.
    let nearby_rows = observed_text_rows(&buffer, cursor_line)?;
    if nearby_rows.is_empty() {
        return Ok(None);
    }

    let (tracked_cursor_line, tracked_nearby_rows) = match tracked_location {
        Some(location)
            if location.buffer_handle == buffer_handle
                && location.line >= 1
                && location.line != cursor_line =>
        {
            // Surprising: edits above the cursor renumber absolute lines, so we also sample the
            // previously tracked cursor footprint and compare by relative row order.
            let tracked_rows = observed_text_rows(&buffer, location.line)?;
            if tracked_rows.is_empty() {
                (None, None)
            } else {
                (Some(location.line), Some(tracked_rows))
            }
        }
        Some(location)
            if location.buffer_handle == buffer_handle
                && location.line >= 1
                && location.line == cursor_line =>
        {
            (Some(cursor_line), Some(nearby_rows.clone()))
        }
        _ => (None, None),
    };

    Ok(Some(CursorTextContext::new(
        buffer_handle,
        changedtick,
        cursor_line,
        nearby_rows,
        tracked_cursor_line,
        tracked_nearby_rows,
    )))
}

#[derive(Debug, Error)]
enum BackgroundProbeMaskError {
    #[error("background probe host bridge call failed: {0}")]
    BridgeCall(#[source] nvim_oxi::Error),
    #[error("background probe mask shape mismatch: {0}")]
    Shape(#[source] LuaParseError),
    #[error("background probe mask decode failed at index {index}: {source}")]
    ValueDecode {
        index: usize,
        #[source]
        source: LuaParseError,
    },
}

fn current_cursor_color_probe_witness(
    mode: &str,
    cursor_position: Option<CursorPosition>,
) -> Result<CursorColorProbeWitness> {
    let buffer = api::get_current_buf();
    if !buffer.is_valid() {
        return Err(nvim_oxi::api::Error::Other("current buffer invalid".into()).into());
    }

    let buffer_handle = i64::from(buffer.handle());
    let changedtick = current_buffer_changedtick(buffer_handle)?;
    let colorscheme_generation = cursor_color_colorscheme_generation()?;
    // cursor-color sampling can also drift via extmarks or semantic-token overlays
    // without a changedtick bump. Keep the cache tied to the cheap shell reads we can afford on
    // every probe edge for now; if telemetry still shows stale reuse, widen the key instead of
    // collapsing the deferred effect boundary back into a synchronous shell read.
    Ok(CursorColorProbeWitness::new(
        buffer_handle,
        changedtick,
        mode.to_owned(),
        cursor_position,
        colorscheme_generation,
    ))
}

fn cursor_color_ready_event(
    payload: &RequestProbeEffect,
    reuse: ProbeReuse,
    sample: Option<CursorColorSample>,
) -> CoreEvent {
    CoreEvent::ProbeReported(ProbeReportedEvent::CursorColorReady {
        observation_id: payload.observation_basis.observation_id(),
        probe_request_id: payload.probe_request_id,
        reuse,
        sample,
    })
}

fn cursor_color_failed_event(payload: &RequestProbeEffect) -> CoreEvent {
    CoreEvent::ProbeReported(ProbeReportedEvent::CursorColorFailed {
        observation_id: payload.observation_basis.observation_id(),
        probe_request_id: payload.probe_request_id,
        failure: ProbeFailure::ShellReadFailed,
    })
}

fn collect_observation_basis(
    payload: &RequestObservationBaseEffect,
) -> Result<(ObservationBasis, ObservationMotion)> {
    let observed_at = to_core_millis(now_ms());
    let mode = mode_string();
    let tracked_location = payload.context.tracked_location();
    let cursor_location = cursor_location_for_core_render(tracked_location.clone());
    let viewport = current_viewport_snapshot()?;
    let cursor_position =
        current_core_cursor_position(&mode, payload.context.cursor_position_policy())?;
    let cursor_text_context =
        match current_cursor_text_context(cursor_location.line, tracked_location.as_ref()) {
            Ok(context) => context,
            Err(err) => {
                warn(&format!("cursor text context read failed: {err}"));
                None
            }
        };
    let cursor_color_witness = if payload.request.probes().cursor_color() {
        match current_cursor_color_probe_witness(&mode, cursor_position) {
            Ok(witness) => Some(witness),
            Err(err) => {
                warn(&format!("cursor color witness read failed: {err}"));
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
            cursor_position,
            cursor_location,
            viewport,
        )
        .with_cursor_color_witness(cursor_color_witness)
        .with_cursor_text_context(cursor_text_context),
        ObservationMotion::new(scroll_shift),
    ))
}

fn batch_background_allowed_mask(
    viewport: ViewportSnapshot,
    chunk: BackgroundProbeChunk,
) -> std::result::Result<Vec<bool>, BackgroundProbeMaskError> {
    let start_row = chunk.start_row().value();
    if start_row == 0 || start_row > viewport.max_row.value() {
        return Ok(Vec::new());
    }

    let row_count = chunk.row_count().min(
        viewport
            .max_row
            .value()
            .saturating_sub(start_row)
            .saturating_add(1),
    );
    let width = usize::try_from(viewport.max_col.value()).map_err(|_| {
        BackgroundProbeMaskError::Shape(crate::lua::invalid_key_error(
            "background_probe_mask",
            "viewport width that fits in usize",
        ))
    })?;
    let row_count_usize = usize::try_from(row_count).map_err(|_| {
        BackgroundProbeMaskError::Shape(crate::lua::invalid_key_error(
            "background_probe_mask",
            "row count that fits in usize",
        ))
    })?;
    let expected_len = width.checked_mul(row_count_usize).ok_or_else(|| {
        BackgroundProbeMaskError::Shape(crate::lua::invalid_key_error(
            "background_probe_mask",
            "mask length that fits in usize",
        ))
    })?;
    if expected_len == 0 {
        return Ok(Vec::new());
    }

    let request = Array::from_iter([
        Object::from(i64::from(start_row)),
        Object::from(i64::from(row_count)),
        Object::from(i64::from(viewport.max_col.value())),
        Object::from(BRAILLE_CODE_MIN),
        Object::from(BRAILLE_CODE_MAX),
        Object::from(OCTANT_CODE_MIN),
        Object::from(OCTANT_CODE_MAX),
    ]);
    let host_bridge = installed_host_bridge()
        .map_err(nvim_oxi::Error::from)
        .map_err(BackgroundProbeMaskError::BridgeCall)?;
    let value = host_bridge
        .background_allowed_mask(request)
        .map_err(nvim_oxi::Error::from)
        .map_err(BackgroundProbeMaskError::BridgeCall)?;
    let values = parse_indexed_objects_typed("background_probe_mask", value, Some(expected_len))
        .map_err(BackgroundProbeMaskError::Shape)?;

    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            bool_from_object_typed("background_probe_mask", value)
                .map_err(|source| BackgroundProbeMaskError::ValueDecode { index, source })
        })
        .collect()
}

fn collect_cursor_color_report(payload: &RequestProbeEffect) -> CoreEvent {
    let Some(expected_witness) = payload.observation_basis.cursor_color_witness() else {
        warn("cursor color probe missing witness");
        return cursor_color_failed_event(payload);
    };
    let current_mode = mode_string();
    let current_position =
        match current_core_cursor_position(&current_mode, payload.cursor_position_policy) {
            Ok(position) => position,
            Err(err) => {
                warn(&format!("cursor color probe failed: {err}"));
                return cursor_color_failed_event(payload);
            }
        };
    let current_witness = match current_cursor_color_probe_witness(&current_mode, current_position)
    {
        Ok(witness) => witness,
        Err(err) => {
            warn(&format!("cursor color probe witness read failed: {err}"));
            return cursor_color_failed_event(payload);
        }
    };

    if &current_witness != expected_witness {
        return cursor_color_ready_event(payload, ProbeReuse::RefreshRequired, None);
    }

    match cached_cursor_color_sample(expected_witness) {
        Ok(CursorColorCacheLookup::Hit(sample)) => {
            return cursor_color_ready_event(payload, ProbeReuse::Exact, sample);
        }
        Ok(CursorColorCacheLookup::Miss) => {}
        Err(err) => {
            warn(&format!("cursor color cache read failed: {err}"));
            return cursor_color_failed_event(payload);
        }
    }

    match sampled_cursor_color_at_current_position() {
        Ok(sample) => {
            let sample = sample.map(CursorColorSample::new);
            if let Err(err) = store_cursor_color_sample(current_witness, sample.clone()) {
                warn(&format!("cursor color cache write failed: {err}"));
            }
            cursor_color_ready_event(payload, ProbeReuse::Exact, sample)
        }
        Err(err) => {
            warn(&format!("cursor color sampling failed: {err}"));
            cursor_color_failed_event(payload)
        }
    }
}

fn collect_background_report(payload: &RequestProbeEffect) -> CoreEvent {
    let current_viewport = match current_viewport_snapshot() {
        Ok(viewport) => viewport,
        Err(err) => {
            warn(&format!("background probe viewport read failed: {err}"));
            return CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundFailed {
                observation_id: payload.observation_basis.observation_id(),
                probe_request_id: payload.probe_request_id,
                failure: ProbeFailure::ShellReadFailed,
            });
        }
    };

    if current_viewport != payload.observation_basis.viewport() {
        return CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundReady {
            observation_id: payload.observation_basis.observation_id(),
            probe_request_id: payload.probe_request_id,
            reuse: ProbeReuse::RefreshRequired,
            batch: BackgroundProbeBatch::empty(payload.observation_basis.viewport()),
        });
    }

    let viewport = payload.observation_basis.viewport();
    let Some(chunk) = payload.background_chunk else {
        warn("background probe missing chunk request");
        return CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundFailed {
            observation_id: payload.observation_basis.observation_id(),
            probe_request_id: payload.probe_request_id,
            failure: ProbeFailure::ShellReadFailed,
        });
    };
    let allowed_mask = match batch_background_allowed_mask(viewport, chunk) {
        Ok(allowed_mask) => allowed_mask,
        Err(err) => {
            warn(&format!("background sampling failed: {err}"));
            return CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundFailed {
                observation_id: payload.observation_basis.observation_id(),
                probe_request_id: payload.probe_request_id,
                failure: ProbeFailure::ShellReadFailed,
            });
        }
    };

    CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundChunkReady {
        observation_id: payload.observation_basis.observation_id(),
        probe_request_id: payload.probe_request_id,
        chunk,
        allowed_mask,
    })
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

pub(crate) fn execute_core_request_probe_effect(payload: &RequestProbeEffect) -> Vec<CoreEvent> {
    let event = match payload.kind {
        ProbeKind::CursorColor => collect_cursor_color_report(payload),
        ProbeKind::Background => collect_background_report(payload),
    };
    vec![event]
}

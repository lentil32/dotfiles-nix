use super::super::cursor::{
    cursor_position_for_mode, mode_string, sampled_cursor_color_at_current_position,
};
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
    ObservationBasis, ObservationMotion, ProbeFailure, ProbeKind, ProbeReuse,
};
use crate::core::types::{CursorCol, CursorPosition, CursorRow, ViewportSnapshot};
use crate::draw::{
    BRAILLE_CODE_MAX, BRAILLE_CODE_MIN, OCTANT_CODE_MAX, OCTANT_CODE_MIN, editor_bounds,
};
use crate::lua::{i64_from_object, parse_indexed_objects};
use crate::types::ScreenCell;
use nvim_oxi::{Array, Object, Result, api};

const SCREENCHAR_BATCH_LUAEVAL_EXPR: &str = r"(function(cells)
  local result = {}
  for i, cell in ipairs(cells) do
    result[i] = vim.fn.screenchar(cell[1], cell[2])
  end
  return result
end)(_A)";

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
    let colorscheme_generation = cursor_color_colorscheme_generation();
    // Comment: cursor-color sampling can also drift via extmarks or semantic-token overlays
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
    let cursor_location = cursor_location_for_core_render(payload.context.tracked_location());
    let viewport = current_viewport_snapshot()?;
    let cursor_position =
        current_core_cursor_position(&mode, payload.context.cursor_position_policy())?;
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
            maybe_scroll_shift_for_core_event(&window, payload.context, cursor_location)?
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
        .with_cursor_color_witness(cursor_color_witness),
        ObservationMotion::new(scroll_shift),
    ))
}

fn background_char_code_is_allowed(code: i64) -> bool {
    let is_space = code == 32;
    let is_braille = (BRAILLE_CODE_MIN..=BRAILLE_CODE_MAX).contains(&code);
    let is_octant = (OCTANT_CODE_MIN..=OCTANT_CODE_MAX).contains(&code);
    is_space || is_braille || is_octant
}

fn background_probe_cells_for_chunk(
    viewport: ViewportSnapshot,
    chunk: BackgroundProbeChunk,
) -> Vec<ScreenCell> {
    let capacity = usize::try_from(chunk.row_count())
        .ok()
        .and_then(|rows| {
            usize::try_from(viewport.max_col.value())
                .ok()
                .and_then(|cols| rows.checked_mul(cols))
        })
        .unwrap_or(0);
    let mut cells = Vec::with_capacity(capacity);
    let start_row = chunk.start_row().value();
    let end_row = start_row
        .saturating_add(chunk.row_count().saturating_sub(1))
        .min(viewport.max_row.value());
    for row in start_row..=end_row {
        for col in 1..=viewport.max_col.value() {
            let Some(cell) = ScreenCell::new(i64::from(row), i64::from(col)) else {
                continue;
            };
            cells.push(cell);
        }
    }
    cells
}

fn batch_screen_char_codes(cells: &[ScreenCell]) -> Option<Vec<i64>> {
    if cells.is_empty() {
        return Some(Vec::new());
    }

    let request = Array::from_iter(cells.iter().map(|cell| {
        Object::from(Array::from_iter([
            Object::from(cell.row()),
            Object::from(cell.col()),
        ]))
    }));
    let args = Array::from_iter([
        Object::from(SCREENCHAR_BATCH_LUAEVAL_EXPR),
        Object::from(request),
    ]);
    let value = api::call_function("luaeval", args).ok()?;
    let values = parse_indexed_objects("screenchar_batch", value, Some(cells.len())).ok()?;

    values
        .into_iter()
        .map(|value| i64_from_object("screenchar_batch", value).ok())
        .collect()
}

fn collect_cursor_color_report(payload: RequestProbeEffect) -> CoreEvent {
    let Some(expected_witness) = payload.observation_basis.cursor_color_witness() else {
        warn("cursor color probe missing witness");
        return cursor_color_failed_event(&payload);
    };
    let current_mode = mode_string();
    let current_position =
        match current_core_cursor_position(&current_mode, payload.cursor_position_policy) {
            Ok(position) => position,
            Err(err) => {
                warn(&format!("cursor color probe failed: {err}"));
                return cursor_color_failed_event(&payload);
            }
        };
    let current_witness = match current_cursor_color_probe_witness(&current_mode, current_position)
    {
        Ok(witness) => witness,
        Err(err) => {
            warn(&format!("cursor color probe witness read failed: {err}"));
            return cursor_color_failed_event(&payload);
        }
    };

    if &current_witness != expected_witness {
        return cursor_color_ready_event(&payload, ProbeReuse::RefreshRequired, None);
    }

    match cached_cursor_color_sample(expected_witness) {
        CursorColorCacheLookup::Hit(sample) => {
            return cursor_color_ready_event(&payload, ProbeReuse::Exact, sample);
        }
        CursorColorCacheLookup::Miss => {}
    }

    match sampled_cursor_color_at_current_position() {
        Ok(sample) => {
            let sample = sample.map(CursorColorSample::new);
            store_cursor_color_sample(current_witness, sample.clone());
            cursor_color_ready_event(&payload, ProbeReuse::Exact, sample)
        }
        Err(err) => {
            warn(&format!("cursor color sampling failed: {err}"));
            cursor_color_failed_event(&payload)
        }
    }
}

fn collect_background_report(payload: RequestProbeEffect) -> CoreEvent {
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
    let cells = background_probe_cells_for_chunk(viewport, chunk);
    let Some(char_codes) = batch_screen_char_codes(&cells) else {
        warn("background sampling failed");
        return CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundFailed {
            observation_id: payload.observation_basis.observation_id(),
            probe_request_id: payload.probe_request_id,
            failure: ProbeFailure::ShellReadFailed,
        });
    };

    let allowed_mask = char_codes
        .into_iter()
        .map(background_char_code_is_allowed)
        .collect::<Vec<_>>();

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

pub(crate) fn execute_core_request_probe_effect(payload: RequestProbeEffect) -> Vec<CoreEvent> {
    let event = match payload.kind {
        ProbeKind::CursorColor => collect_cursor_color_report(payload),
        ProbeKind::Background => collect_background_report(payload),
    };
    vec![event]
}

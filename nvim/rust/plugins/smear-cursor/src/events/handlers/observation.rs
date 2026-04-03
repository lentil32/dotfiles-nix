use super::super::cursor::cursor_position_for_mode;
use super::super::cursor::mode_string;
use super::super::cursor::sampled_cursor_color_at_current_position;
use super::super::host_bridge::installed_host_bridge;
use super::super::logging::warn;
use super::super::probe_cache::CursorColorCacheLookup;
use super::super::probe_cache::CursorTextContextCacheKey;
use super::super::probe_cache::CursorTextContextCacheLookup;
use super::super::runtime::cached_cursor_color_sample;
use super::super::runtime::cached_cursor_text_context;
use super::super::runtime::cursor_color_colorscheme_generation;
use super::super::runtime::now_ms;
use super::super::runtime::store_cursor_color_sample;
use super::super::runtime::store_cursor_text_context;
use super::super::runtime::to_core_millis;
use super::viewport::cursor_location_for_core_render;
use super::viewport::maybe_scroll_shift_for_core_event;
use crate::core::effect::CursorPositionReadPolicy;
use crate::core::effect::RequestObservationBaseEffect;
use crate::core::effect::RequestProbeEffect;
use crate::core::event::Event as CoreEvent;
use crate::core::event::ObservationBaseCollectedEvent;
use crate::core::event::ProbeReportedEvent;
use crate::core::state::BackgroundProbeBatch;
use crate::core::state::BackgroundProbeChunk;
use crate::core::state::BackgroundProbeChunkMask;
use crate::core::state::CursorColorProbeWitness;
use crate::core::state::CursorColorSample;
use crate::core::state::CursorTextContext;
use crate::core::state::ObservationBasis;
use crate::core::state::ObservationMotion;
use crate::core::state::ObservedTextRow;
use crate::core::state::ProbeFailure;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeReuse;
use crate::core::types::CursorCol;
use crate::core::types::CursorPosition;
use crate::core::types::CursorRow;
use crate::core::types::Generation;
use crate::core::types::ViewportSnapshot;
use crate::draw::BRAILLE_CODE_MAX;
use crate::draw::BRAILLE_CODE_MIN;
use crate::draw::OCTANT_CODE_MAX;
use crate::draw::OCTANT_CODE_MIN;
use crate::draw::editor_bounds;
use crate::lua::LuaParseError;
use crate::lua::i64_from_object;
use crate::lua::parse_indexed_objects_typed;
use crate::lua::u8_from_object_typed;
use nvim_oxi::Array;
use nvim_oxi::Object;
use nvim_oxi::Result;
use nvim_oxi::api;
use std::sync::Arc;
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

fn observed_text_rows(buffer: &api::Buffer, center_line: i64) -> Result<Arc<[ObservedTextRow]>> {
    if center_line < 1 {
        return Ok(Arc::default());
    }

    let start_line = center_line.saturating_sub(1).max(1);
    let end_line = center_line.saturating_add(1);
    let start_index = usize::try_from(start_line.saturating_sub(1)).ok();
    let end_index = usize::try_from(end_line).ok();
    let (Some(start_index), Some(end_index)) = (start_index, end_index) else {
        return Ok(Arc::default());
    };

    let rows = buffer
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
        .collect::<Result<Vec<_>>>()?;
    Ok(rows.into())
}

fn tracked_cursor_text_context_line(
    buffer_handle: i64,
    tracked_location: Option<&crate::state::CursorLocation>,
) -> Option<i64> {
    tracked_location.and_then(|location| {
        (location.buffer_handle == buffer_handle && location.line >= 1).then_some(location.line)
    })
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
    let tracked_cursor_line = tracked_cursor_text_context_line(buffer_handle, tracked_location);
    let cache_key = CursorTextContextCacheKey::new(
        buffer_handle,
        changedtick,
        cursor_line,
        tracked_cursor_line,
    );
    match cached_cursor_text_context(&cache_key) {
        Ok(CursorTextContextCacheLookup::Hit(context)) => return Ok(context),
        Ok(CursorTextContextCacheLookup::Miss) => {}
        Err(err) => warn(&format!("cursor text context cache read failed: {err}")),
    }

    // Surprising: embedded Neovim does not expose Neovide's redraw grid here, so semantic
    // mutation detection uses narrow buffer-line snapshots plus changedtick instead of UI cells.
    let nearby_rows = observed_text_rows(&buffer, cursor_line)?;
    let context = if nearby_rows.is_empty() {
        None
    } else {
        let (tracked_cursor_line, tracked_nearby_rows) = match tracked_cursor_line {
            Some(tracked_cursor_line) if tracked_cursor_line != cursor_line => {
                // Surprising: edits above the cursor renumber absolute lines, so we also sample the
                // previously tracked cursor footprint and compare by relative row order.
                let tracked_rows = observed_text_rows(&buffer, tracked_cursor_line)?;
                if tracked_rows.is_empty() {
                    (None, None)
                } else {
                    (Some(tracked_cursor_line), Some(tracked_rows))
                }
            }
            Some(_) => (Some(cursor_line), Some(Arc::clone(&nearby_rows))),
            None => (None, None),
        };

        Some(CursorTextContext::from_shared(
            buffer_handle,
            changedtick,
            cursor_line,
            nearby_rows,
            tracked_cursor_line,
            tracked_nearby_rows,
        ))
    };
    if let Err(err) = store_cursor_text_context(cache_key, context.clone()) {
        warn(&format!("cursor text context cache write failed: {err}"));
    }
    Ok(context)
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

fn validate_cursor_color_probe_witness(
    expected_witness: &CursorColorProbeWitness,
    current_mode: &str,
    current_position: Option<CursorPosition>,
    current_buffer_handle: i64,
    current_changedtick: u64,
    current_colorscheme_generation: Generation,
) -> ProbeReuse {
    if expected_witness.mode() != current_mode
        || expected_witness.cursor_position() != current_position
        || expected_witness.buffer_handle() != current_buffer_handle
        || expected_witness.changedtick() != current_changedtick
        || expected_witness.colorscheme_generation() != current_colorscheme_generation
    {
        ProbeReuse::RefreshRequired
    } else {
        ProbeReuse::Exact
    }
}

fn current_cursor_color_probe_reuse(
    expected_witness: &CursorColorProbeWitness,
    policy: CursorPositionReadPolicy,
) -> Result<ProbeReuse> {
    let current_mode = mode_string();
    if current_mode != expected_witness.mode() {
        return Ok(ProbeReuse::RefreshRequired);
    }

    let current_position = current_core_cursor_position(&current_mode, policy)?;
    if current_position != expected_witness.cursor_position() {
        return Ok(ProbeReuse::RefreshRequired);
    }

    let current_buffer = api::get_current_buf();
    if !current_buffer.is_valid() {
        return Err(nvim_oxi::api::Error::Other("current buffer invalid".into()).into());
    }

    let current_buffer_handle = i64::from(current_buffer.handle());
    let current_changedtick = current_buffer_changedtick(current_buffer_handle)?;
    let current_colorscheme_generation = cursor_color_colorscheme_generation()?;
    Ok(validate_cursor_color_probe_witness(
        expected_witness,
        &current_mode,
        current_position,
        current_buffer_handle,
        current_changedtick,
        current_colorscheme_generation,
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
    chunk: &BackgroundProbeChunk,
) -> std::result::Result<BackgroundProbeChunkMask, BackgroundProbeMaskError> {
    let cells = chunk.cells();
    if cells.is_empty() {
        return Ok(BackgroundProbeChunkMask::from_allowed_mask(&[]));
    }
    if cells.iter().any(|cell| {
        let Ok(row) = u32::try_from(cell.row()) else {
            return true;
        };
        let Ok(col) = u32::try_from(cell.col()) else {
            return true;
        };
        row == 0 || col == 0 || row > viewport.max_row.value() || col > viewport.max_col.value()
    }) {
        return Err(BackgroundProbeMaskError::Shape(
            crate::lua::invalid_key_error(
                "background_probe_mask",
                "chunk cells within viewport bounds",
            ),
        ));
    }

    let expected_len = cells.len();
    let expected_packed_len = expected_len / 8 + usize::from(!expected_len.is_multiple_of(8));
    let mut request = Vec::with_capacity(4 + expected_len.saturating_mul(2));
    request.push(Object::from(BRAILLE_CODE_MIN));
    request.push(Object::from(BRAILLE_CODE_MAX));
    request.push(Object::from(OCTANT_CODE_MIN));
    request.push(Object::from(OCTANT_CODE_MAX));
    for cell in cells {
        request.push(Object::from(cell.row()));
        request.push(Object::from(cell.col()));
    }

    let request = Array::from_iter(request);
    let host_bridge = installed_host_bridge()
        .map_err(nvim_oxi::Error::from)
        .map_err(BackgroundProbeMaskError::BridgeCall)?;
    let value = host_bridge
        .background_allowed_mask(request)
        .map_err(nvim_oxi::Error::from)
        .map_err(BackgroundProbeMaskError::BridgeCall)?;
    let values =
        parse_indexed_objects_typed("background_probe_mask", value, Some(expected_packed_len))
            .map_err(BackgroundProbeMaskError::Shape)?;
    let packed = values
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            u8_from_object_typed("background_probe_mask", value)
                .map_err(|source| BackgroundProbeMaskError::ValueDecode { index, source })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;

    BackgroundProbeChunkMask::from_packed_bytes(expected_len, packed).ok_or_else(|| {
        BackgroundProbeMaskError::Shape(crate::lua::invalid_key_error(
            "background_probe_mask",
            "packed byte array",
        ))
    })
}

fn collect_cursor_color_report(payload: &RequestProbeEffect, same_reducer_wave: bool) -> CoreEvent {
    let Some(expected_witness) = payload.observation_basis.cursor_color_witness() else {
        warn("cursor color probe missing witness");
        return cursor_color_failed_event(payload);
    };
    if !same_reducer_wave {
        let reuse = match current_cursor_color_probe_reuse(
            expected_witness,
            payload.cursor_position_policy,
        ) {
            Ok(reuse) => reuse,
            Err(err) => {
                warn(&format!("cursor color probe witness read failed: {err}"));
                return cursor_color_failed_event(payload);
            }
        };
        if reuse == ProbeReuse::RefreshRequired {
            return cursor_color_ready_event(payload, ProbeReuse::RefreshRequired, None);
        }
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

    match sampled_cursor_color_at_current_position(expected_witness.colorscheme_generation()) {
        Ok(sample) => {
            let sample = sample.map(CursorColorSample::new);
            if let Err(err) = store_cursor_color_sample(expected_witness.clone(), sample) {
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
    let Some(chunk) = payload.background_chunk.as_ref() else {
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
        chunk: chunk.clone(),
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
    execute_core_request_probe_effect_with_reuse(payload, false)
}

pub(crate) fn execute_core_request_probe_effect_same_reducer_wave(
    payload: &RequestProbeEffect,
) -> Vec<CoreEvent> {
    execute_core_request_probe_effect_with_reuse(payload, true)
}

fn execute_core_request_probe_effect_with_reuse(
    payload: &RequestProbeEffect,
    same_reducer_wave: bool,
) -> Vec<CoreEvent> {
    let event = match payload.kind {
        ProbeKind::CursorColor => collect_cursor_color_report(payload, same_reducer_wave),
        ProbeKind::Background => collect_background_report(payload),
    };
    vec![event]
}

#[cfg(test)]
mod tests {
    use super::validate_cursor_color_probe_witness;
    use crate::core::state::CursorColorProbeWitness;
    use crate::core::state::ProbeReuse;
    use crate::core::types::CursorCol;
    use crate::core::types::CursorPosition;
    use crate::core::types::CursorRow;
    use crate::core::types::Generation;

    fn cursor(row: u32, col: u32) -> CursorPosition {
        CursorPosition {
            row: CursorRow(row),
            col: CursorCol(col),
        }
    }

    fn witness(
        buffer_handle: i64,
        changedtick: u64,
        mode: &str,
        cursor_position: Option<CursorPosition>,
        colorscheme_generation: u64,
    ) -> CursorColorProbeWitness {
        CursorColorProbeWitness::new(
            buffer_handle,
            changedtick,
            mode.to_string(),
            cursor_position,
            Generation::new(colorscheme_generation),
        )
    }

    #[test]
    fn validate_cursor_color_probe_witness_reuses_captured_snapshot_when_shell_reads_match() {
        let expected = witness(22, 14, "n", Some(cursor(7, 8)), 3);

        assert_eq!(
            validate_cursor_color_probe_witness(
                &expected,
                "n",
                Some(cursor(7, 8)),
                22,
                14,
                Generation::new(3),
            ),
            ProbeReuse::Exact,
        );
    }

    #[test]
    fn validate_cursor_color_probe_witness_requires_refresh_when_snapshot_goes_stale() {
        let expected = witness(22, 14, "n", Some(cursor(7, 8)), 3);

        assert_eq!(
            validate_cursor_color_probe_witness(
                &expected,
                "n",
                Some(cursor(7, 8)),
                22,
                15,
                Generation::new(3),
            ),
            ProbeReuse::RefreshRequired,
        );
        assert_eq!(
            validate_cursor_color_probe_witness(
                &expected,
                "i",
                Some(cursor(7, 8)),
                22,
                14,
                Generation::new(3),
            ),
            ProbeReuse::RefreshRequired,
        );
    }
}

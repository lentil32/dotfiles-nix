use super::base::current_viewport_snapshot;
use crate::core::effect::RequestProbeEffect;
use crate::core::event::Event as CoreEvent;
use crate::core::event::ProbeReportedEvent;
use crate::core::state::BackgroundProbeBatch;
use crate::core::state::BackgroundProbeChunk;
use crate::core::state::BackgroundProbeChunkMask;
use crate::core::state::ProbeFailure;
use crate::draw::BRAILLE_CODE_MAX;
use crate::draw::BRAILLE_CODE_MIN;
use crate::draw::OCTANT_CODE_MAX;
use crate::draw::OCTANT_CODE_MIN;
use crate::events::host_bridge::installed_host_bridge;
use crate::lua::LuaParseError;
use crate::lua::parse_indexed_objects_typed;
use crate::lua::u8_from_object_typed;
use nvim_oxi::Array;
use nvim_oxi::Object;
use thiserror::Error;

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

fn batch_background_allowed_mask(
    viewport: crate::core::types::ViewportSnapshot,
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

pub(super) fn collect_background_report(payload: &RequestProbeEffect) -> CoreEvent {
    let current_viewport = match current_viewport_snapshot() {
        Ok(viewport) => viewport,
        Err(err) => {
            crate::events::logging::warn(&format!("background probe viewport read failed: {err}"));
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
            reuse: crate::core::state::ProbeReuse::RefreshRequired,
            batch: BackgroundProbeBatch::empty(payload.observation_basis.viewport()),
        });
    }

    let viewport = payload.observation_basis.viewport();
    let Some(chunk) = payload.background_chunk.as_ref() else {
        crate::events::logging::warn("background probe missing chunk request");
        return CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundFailed {
            observation_id: payload.observation_basis.observation_id(),
            probe_request_id: payload.probe_request_id,
            failure: ProbeFailure::ShellReadFailed,
        });
    };
    let allowed_mask = match batch_background_allowed_mask(viewport, chunk) {
        Ok(allowed_mask) => allowed_mask,
        Err(err) => {
            crate::events::logging::warn(&format!("background sampling failed: {err}"));
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

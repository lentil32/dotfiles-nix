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
use crate::events::RuntimeAccessError;
use crate::events::host_bridge::installed_host_bridge;
use crate::events::runtime::reclaim_background_probe_request_scratch;
use crate::events::runtime::take_background_probe_request_scratch;
use crate::lua::LuaParseError;
use crate::lua::parse_indexed_objects_typed;
use crate::lua::u8_from_object_typed;
use crate::position::ViewportBounds;
use nvim_oxi::Array;
use nvim_oxi::Object;
use thiserror::Error;

#[derive(Debug, Error)]
enum BackgroundProbeMaskError {
    #[error("background probe runtime access failed: {0}")]
    RuntimeAccess(#[source] RuntimeAccessError),
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

fn background_probe_request_len(chunk: &BackgroundProbeChunk) -> usize {
    4 + chunk.len().saturating_mul(2)
}

fn populate_background_probe_request(request: &mut Vec<Object>, chunk: &BackgroundProbeChunk) {
    request.clear();
    request.reserve(background_probe_request_len(chunk).saturating_sub(request.len()));
    request.push(Object::from(BRAILLE_CODE_MIN));
    request.push(Object::from(BRAILLE_CODE_MAX));
    request.push(Object::from(OCTANT_CODE_MIN));
    request.push(Object::from(OCTANT_CODE_MAX));
    for cell in chunk.iter_cells() {
        request.push(Object::from(cell.row()));
        request.push(Object::from(cell.col()));
    }
}

fn with_background_probe_request_scratch<R>(
    body: impl FnOnce(&mut Vec<Object>) -> std::result::Result<R, BackgroundProbeMaskError>,
) -> std::result::Result<R, BackgroundProbeMaskError> {
    let mut request =
        take_background_probe_request_scratch().map_err(BackgroundProbeMaskError::RuntimeAccess)?;
    let result = body(&mut request);
    let reclaim = reclaim_background_probe_request_scratch(request)
        .map_err(BackgroundProbeMaskError::RuntimeAccess);
    match (result, reclaim) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(err), Ok(())) => Err(err),
        (_, Err(err)) => Err(err),
    }
}

fn batch_background_allowed_mask(
    viewport: ViewportBounds,
    chunk: &BackgroundProbeChunk,
) -> std::result::Result<BackgroundProbeChunkMask, BackgroundProbeMaskError> {
    if chunk.len() == 0 {
        return Ok(BackgroundProbeChunkMask::from_allowed_mask(&[]));
    }
    if chunk
        .iter_cells()
        .any(|cell| cell.row() > viewport.max_row() || cell.col() > viewport.max_col())
    {
        return Err(BackgroundProbeMaskError::Shape(
            crate::lua::invalid_key_error(
                "background_probe_mask",
                "chunk cells within viewport bounds",
            ),
        ));
    }

    let expected_len = chunk.len();
    let expected_packed_len = expected_len / 8 + usize::from(!expected_len.is_multiple_of(8));
    with_background_probe_request_scratch(|request| {
        populate_background_probe_request(request, chunk);
        let request = Array::from_iter(request.drain(..));
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
    })
}

pub(super) fn collect_background_report(payload: &RequestProbeEffect) -> CoreEvent {
    let current_viewport = match current_viewport_snapshot() {
        Ok(viewport) => viewport,
        Err(err) => {
            crate::events::logging::warn(&format!("background probe viewport read failed: {err}"));
            return CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundFailed {
                observation_id: payload.observation_id,
                failure: ProbeFailure::ShellReadFailed,
            });
        }
    };

    if current_viewport != payload.observation_basis.viewport() {
        return CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundReady {
            observation_id: payload.observation_id,
            reuse: crate::core::state::ProbeReuse::RefreshRequired,
            batch: BackgroundProbeBatch::empty(),
        });
    }

    let viewport = payload.observation_basis.viewport();
    let Some(chunk) = payload.background_chunk.as_ref() else {
        crate::events::logging::warn("background probe missing chunk request");
        return CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundFailed {
            observation_id: payload.observation_id,
            failure: ProbeFailure::ShellReadFailed,
        });
    };
    let allowed_mask = match batch_background_allowed_mask(viewport, chunk) {
        Ok(allowed_mask) => allowed_mask,
        Err(err) => {
            crate::events::logging::warn(&format!("background sampling failed: {err}"));
            return CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundFailed {
                observation_id: payload.observation_id,
                failure: ProbeFailure::ShellReadFailed,
            });
        }
    };

    CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundChunkReady {
        observation_id: payload.observation_id,
        chunk: chunk.clone(),
        allowed_mask,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::BackgroundProbePlan;
    use crate::core::state::BackgroundProbeProgress;
    use crate::lua::i64_from_object_typed;
    use crate::position::ScreenCell;
    use pretty_assertions::assert_eq;

    #[test]
    fn populate_background_probe_request_writes_header_and_cell_pairs() {
        let plan = BackgroundProbePlan::from_cells(vec![
            ScreenCell::new(7, 8).expect("cell"),
            ScreenCell::new(9, 10).expect("cell"),
        ]);
        let chunk = BackgroundProbeProgress::new(plan)
            .next_chunk()
            .expect("chunk should build");
        let mut request = Vec::new();

        populate_background_probe_request(&mut request, &chunk);

        let values = request
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                i64_from_object_typed("background_probe_request", value).unwrap_or_else(|err| {
                    panic!("request value {index} should decode to an integer: {err}")
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            values,
            vec![
                BRAILLE_CODE_MIN,
                BRAILLE_CODE_MAX,
                OCTANT_CODE_MIN,
                OCTANT_CODE_MAX,
                7,
                8,
                9,
                10,
            ]
        );
    }

    #[test]
    fn background_probe_request_scratch_reuses_larger_shell_buffer() {
        let initial_capacity = crate::events::runtime::read_shell_state(|state| {
            state.background_probe_request_scratch_capacity()
        })
        .expect("shell state should be readable");
        let scratch_capacity = initial_capacity.max(32);
        let mut scratch =
            take_background_probe_request_scratch().expect("scratch should be available");
        scratch.reserve(scratch_capacity.saturating_sub(scratch.len()));
        let scratch_ptr = scratch.as_ptr();
        let scratch_capacity = scratch.capacity();

        reclaim_background_probe_request_scratch(scratch).expect("scratch should be reclaimable");

        let scratch = take_background_probe_request_scratch().expect("scratch should be available");
        assert_eq!(scratch.capacity(), scratch_capacity);
        assert_eq!(scratch.as_ptr(), scratch_ptr);
        reclaim_background_probe_request_scratch(scratch).expect("scratch should be reclaimable");
    }
}

use super::super::HostBridgeRevision;
use super::super::HostBridgeState;
use super::super::RealCursorVisibility;
use super::super::ShellState;
use super::super::cursor::BufferMetadata;
use super::super::policy::BufferEventPolicy;
use super::super::policy::buffer_event_policy_from_snapshot;
use super::super::probe_cache::CachedCursorColorProbeSample;
use super::super::probe_cache::ConcealCacheKey;
use super::super::probe_cache::ConcealCacheLookup;
use super::super::probe_cache::ConcealDeltaCacheKey;
use super::super::probe_cache::ConcealDeltaCacheLookup;
use super::super::probe_cache::ConcealRegion;
use super::super::probe_cache::ConcealScreenCell;
use super::super::probe_cache::ConcealScreenCellCacheKey;
use super::super::probe_cache::ConcealScreenCellCacheLookup;
use super::super::probe_cache::CursorTextContextCacheKey;
use super::super::probe_cache::CursorTextContextCacheLookup;
use super::EditorViewportSnapshot;
use super::IngressReadSnapshot;
use super::RuntimeAccessResult;
use super::cell::restore_shell_state;
use super::cell::take_shell_state;
use super::editor_viewport::EditorViewportCache;
use super::recovery::RuntimeRecoveryPlan;
use super::timers::capture_runtime_timer_bridge_recovery_state;
use super::timers::now_ms;
use crate::core::effect::ProbePolicy;
use crate::core::state::CursorColorProbeWitness;
use crate::core::state::CursorColorSample;
use crate::core::state::CursorTextContext;
use crate::core::types::Generation;
use crate::events::policy::BufferPerfTelemetryCache;
use crate::events::probe_cache::ProbeCacheState;
use crate::host::BufferHandle;
use crate::host::NamespaceId;
use crate::host::NeovimHost;
use crate::host::api;
use nvim_oxi::Object;
use nvim_oxi::Result;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::panic::resume_unwind;
use std::sync::Arc;

fn with_shell_state_access<R>(
    accessor: impl FnOnce(&mut ShellState) -> R,
) -> RuntimeAccessResult<R> {
    let mut state = take_shell_state()?;
    match catch_unwind(AssertUnwindSafe(|| accessor(&mut state))) {
        Ok(output) => {
            restore_shell_state(state);
            Ok(output)
        }
        Err(panic_payload) => {
            let recovery_state = ShellRecoveryState::capture(&state);
            let timer_recovery_state = capture_runtime_timer_bridge_recovery_state();
            let recovery_plan =
                RuntimeRecoveryPlan::runtime_lane_panic(recovery_state, timer_recovery_state);
            restore_shell_state(state);
            recovery_plan.apply();
            resume_unwind(panic_payload);
        }
    }
}

pub(in crate::events) fn read_shell_state<R>(
    reader: impl FnOnce(&ShellState) -> R,
) -> RuntimeAccessResult<R> {
    with_shell_state_access(|state| reader(state))
}

pub(in crate::events) fn mutate_shell_state<R>(
    mutator: impl FnOnce(&mut ShellState) -> R,
) -> RuntimeAccessResult<R> {
    with_shell_state_access(mutator)
}

pub(super) fn with_editor_viewport_cache<R>(
    accessor: impl FnOnce(&mut EditorViewportCache) -> R,
) -> RuntimeAccessResult<R> {
    mutate_shell_state(|state| accessor(&mut state.editor_viewport_cache))
}

pub(super) fn with_probe_cache<R>(
    accessor: impl FnOnce(&mut ProbeCacheState) -> R,
) -> RuntimeAccessResult<R> {
    mutate_shell_state(|state| accessor(&mut state.probe_cache))
}

pub(super) fn with_buffer_perf_telemetry_cache<R>(
    accessor: impl FnOnce(&mut BufferPerfTelemetryCache) -> R,
) -> RuntimeAccessResult<R> {
    mutate_shell_state(|state| accessor(&mut state.buffer_perf_telemetry_cache))
}

pub(super) fn try_record_telemetry<R>(
    buffer_handle: Option<BufferHandle>,
    recorder: impl FnOnce(&mut BufferPerfTelemetryCache, BufferHandle) -> R,
) -> RuntimeAccessResult<Option<R>> {
    let Some(buffer_handle) = buffer_handle else {
        return Ok(None);
    };

    with_buffer_perf_telemetry_cache(|telemetry| Some(recorder(telemetry, buffer_handle)))
}

pub(crate) fn editor_viewport_for_bounds() -> Result<EditorViewportSnapshot> {
    with_editor_viewport_cache(EditorViewportCache::read_for_bounds)
        .map_err(nvim_oxi::Error::from)?
}

pub(crate) fn editor_viewport_for_command_row() -> Result<EditorViewportSnapshot> {
    with_editor_viewport_cache(EditorViewportCache::read_for_command_row)
        .map_err(nvim_oxi::Error::from)?
}

pub(crate) fn refresh_editor_viewport_cache() -> Result<()> {
    with_editor_viewport_cache(EditorViewportCache::refresh).map_err(nvim_oxi::Error::from)?
}

pub(crate) fn buffer_text_revision(
    buffer_handle: impl Into<BufferHandle>,
) -> RuntimeAccessResult<Generation> {
    let buffer_handle = buffer_handle.into();
    mutate_shell_state(|state| state.buffer_text_revision_cache.current(buffer_handle))
}

pub(crate) fn resolved_current_buffer_event_policy(
    snapshot: &IngressReadSnapshot,
    buffer: &api::Buffer,
) -> Result<BufferEventPolicy> {
    let buffer_handle = BufferHandle::from_buffer(buffer);
    let metadata =
        mutate_shell_state(|state| state.buffer_metadata_cache.read(&NeovimHost, buffer))
            .map_err(nvim_oxi::Error::from)??;
    resolve_buffer_event_policy_for_metadata(snapshot, buffer_handle, &metadata, now_ms())
}

pub(crate) fn resolve_buffer_event_policy_for_metadata(
    snapshot: &IngressReadSnapshot,
    buffer_handle: impl Into<BufferHandle>,
    metadata: &BufferMetadata,
    observed_at_ms: f64,
) -> Result<BufferEventPolicy> {
    let buffer_handle = buffer_handle.into();
    let (previous, telemetry) = read_shell_state(|state| {
        (
            state.buffer_perf_policy_cache.cached_policy(buffer_handle),
            state
                .buffer_perf_telemetry_cache
                .telemetry(buffer_handle)
                .unwrap_or_default(),
        )
    })
    .map_err(nvim_oxi::Error::from)?;
    let policy =
        buffer_event_policy_from_snapshot(snapshot, metadata, previous, telemetry, observed_at_ms);
    mutate_shell_state(|state| {
        state
            .buffer_perf_policy_cache
            .store_policy(buffer_handle, policy);
    })
    .map_err(nvim_oxi::Error::from)?;
    Ok(policy)
}

pub(crate) fn cursor_color_colorscheme_generation() -> RuntimeAccessResult<Generation> {
    with_probe_cache(|cache| cache.colorscheme_generation())
}

pub(crate) fn cursor_color_cache_generation() -> RuntimeAccessResult<Generation> {
    with_probe_cache(|cache| cache.cursor_color_cache_generation())
}

pub(crate) fn cached_cursor_color_sample_for_probe(
    witness: &CursorColorProbeWitness,
    probe_policy: ProbePolicy,
    reuse: crate::core::state::ProbeReuse,
) -> RuntimeAccessResult<Option<CachedCursorColorProbeSample>> {
    with_probe_cache(|cache| {
        cache.cached_cursor_color_sample_for_probe(witness, probe_policy, reuse)
    })
}

pub(crate) fn store_cursor_color_sample(
    witness: CursorColorProbeWitness,
    sample: Option<CursorColorSample>,
) -> RuntimeAccessResult<()> {
    with_probe_cache(|cache| {
        cache.store_cursor_color_sample(witness, sample);
    })
}

pub(crate) fn note_cursor_color_observation_boundary() -> RuntimeAccessResult<()> {
    with_probe_cache(|cache| {
        cache.note_cursor_color_observation_boundary();
    })
}

pub(crate) fn cached_cursor_text_context(
    key: &CursorTextContextCacheKey,
) -> RuntimeAccessResult<CursorTextContextCacheLookup> {
    with_probe_cache(|cache| cache.cached_cursor_text_context(key))
}

pub(crate) fn store_cursor_text_context(
    key: CursorTextContextCacheKey,
    context: Option<CursorTextContext>,
) -> RuntimeAccessResult<()> {
    with_probe_cache(|cache| {
        cache.store_cursor_text_context(key, context);
    })
}

pub(crate) fn cached_conceal_regions(
    key: &ConcealCacheKey,
) -> RuntimeAccessResult<ConcealCacheLookup> {
    with_probe_cache(|cache| cache.cached_conceal_regions(key))
}

pub(crate) fn note_conceal_read_boundary() -> RuntimeAccessResult<()> {
    with_probe_cache(|cache| {
        cache.note_conceal_read_boundary();
    })
}

pub(crate) fn cached_conceal_delta(
    key: &ConcealDeltaCacheKey,
) -> RuntimeAccessResult<ConcealDeltaCacheLookup> {
    with_probe_cache(|cache| cache.cached_conceal_delta(key))
}

pub(crate) fn store_conceal_regions(
    key: ConcealCacheKey,
    scanned_to_col1: i64,
    regions: Arc<[ConcealRegion]>,
) -> RuntimeAccessResult<()> {
    with_probe_cache(|cache| {
        cache.store_conceal_regions(key, scanned_to_col1, regions);
    })
}

pub(crate) fn store_conceal_delta(
    key: ConcealDeltaCacheKey,
    current_col1: i64,
    delta: i64,
) -> RuntimeAccessResult<()> {
    with_probe_cache(|cache| {
        cache.store_conceal_delta(key, current_col1, delta);
    })
}

pub(crate) fn cached_conceal_screen_cell(
    key: &ConcealScreenCellCacheKey,
) -> RuntimeAccessResult<ConcealScreenCellCacheLookup> {
    with_probe_cache(|cache| cache.cached_conceal_screen_cell(key))
}

pub(crate) fn store_conceal_screen_cell(
    key: ConcealScreenCellCacheKey,
    cell: Option<ConcealScreenCell>,
) -> RuntimeAccessResult<()> {
    with_probe_cache(|cache| {
        cache.store_conceal_screen_cell(key, cell);
    })
}

pub(crate) fn note_cursor_color_colorscheme_change() -> RuntimeAccessResult<()> {
    mutate_shell_state(|state| {
        state.note_cursor_color_colorscheme_change();
    })
}

pub(crate) fn take_background_probe_request_scratch() -> RuntimeAccessResult<Vec<Object>> {
    mutate_shell_state(ShellState::take_background_probe_request_scratch)
}

pub(crate) fn reclaim_background_probe_request_scratch(
    scratch: Vec<Object>,
) -> RuntimeAccessResult<()> {
    mutate_shell_state(|state| {
        state.reclaim_background_probe_request_scratch(scratch);
    })
}

pub(crate) fn take_conceal_regions_scratch() -> RuntimeAccessResult<Vec<ConcealRegion>> {
    mutate_shell_state(ShellState::take_conceal_regions_scratch)
}

pub(crate) fn reclaim_conceal_regions_scratch(
    scratch: Vec<ConcealRegion>,
) -> RuntimeAccessResult<()> {
    mutate_shell_state(|state| {
        state.reclaim_conceal_regions_scratch(scratch);
    })
}

pub(crate) fn release_cleanup_cold_shell_storage() -> RuntimeAccessResult<()> {
    mutate_shell_state(|state| {
        state.release_cleanup_cold_storage();
    })
}

pub(crate) fn namespace_id() -> RuntimeAccessResult<Option<NamespaceId>> {
    read_shell_state(ShellState::namespace_id)
}

pub(crate) fn set_namespace_id(namespace_id: NamespaceId) -> RuntimeAccessResult<()> {
    mutate_shell_state(|state| {
        state.set_namespace_id(namespace_id);
    })
}

pub(crate) fn host_bridge_state() -> RuntimeAccessResult<HostBridgeState> {
    read_shell_state(ShellState::host_bridge_state)
}

pub(crate) fn note_host_bridge_verified(revision: HostBridgeRevision) -> RuntimeAccessResult<()> {
    mutate_shell_state(|state| {
        state.note_host_bridge_verified(revision);
    })
}

pub(crate) fn real_cursor_visibility_matches(
    visibility: RealCursorVisibility,
) -> RuntimeAccessResult<bool> {
    read_shell_state(|state| state.real_cursor_visibility() == Some(visibility))
}

pub(crate) fn note_real_cursor_visibility(
    visibility: RealCursorVisibility,
) -> RuntimeAccessResult<()> {
    mutate_shell_state(|state| {
        state.note_real_cursor_visibility(visibility);
    })
}

pub(crate) fn clear_real_cursor_visibility() -> RuntimeAccessResult<()> {
    mutate_shell_state(ShellState::clear_real_cursor_visibility)
}

pub(crate) fn reset_transient_shell_caches() -> RuntimeAccessResult<()> {
    mutate_shell_state(ShellState::reset_transient_caches)
}

pub(crate) fn invalidate_buffer_metadata(
    buffer_handle: impl Into<BufferHandle>,
) -> RuntimeAccessResult<()> {
    let buffer_handle = buffer_handle.into();
    mutate_shell_state(|state| {
        state.invalidate_buffer_metadata(buffer_handle);
    })
}

pub(crate) fn invalidate_buffer_local_probe_caches(
    buffer_handle: impl Into<BufferHandle>,
) -> RuntimeAccessResult<()> {
    let buffer_handle = buffer_handle.into();
    mutate_shell_state(|state| {
        state.invalidate_buffer_local_probe_caches(buffer_handle);
    })
}

pub(crate) fn advance_buffer_text_revision(
    buffer_handle: impl Into<BufferHandle>,
) -> RuntimeAccessResult<()> {
    let buffer_handle = buffer_handle.into();
    mutate_shell_state(|state| {
        state.buffer_text_revision_cache.advance(buffer_handle);
    })
}

pub(crate) fn invalidate_conceal_probe_caches(
    buffer_handle: impl Into<BufferHandle>,
) -> RuntimeAccessResult<()> {
    let buffer_handle = buffer_handle.into();
    mutate_shell_state(|state| {
        state.invalidate_conceal_probe_caches(buffer_handle);
    })
}

pub(crate) fn invalidate_buffer_local_caches(
    buffer_handle: impl Into<BufferHandle>,
) -> RuntimeAccessResult<()> {
    let buffer_handle = buffer_handle.into();
    mutate_shell_state(|state| {
        state.invalidate_buffer_local_caches(buffer_handle);
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ShellRecoveryState {
    pub(super) namespace_id: Option<NamespaceId>,
    pub(super) host_bridge_state: HostBridgeState,
}

impl ShellRecoveryState {
    fn capture(state: &ShellState) -> Self {
        Self {
            namespace_id: state.namespace_id(),
            host_bridge_state: state.host_bridge_state(),
        }
    }
}

fn restore_recovered_shell_state(state: &mut ShellState, recovery_state: ShellRecoveryState) {
    *state = ShellState::default();
    state.host_bridge_state = recovery_state.host_bridge_state;
}

pub(super) fn capture_runtime_shell_recovery_state() -> ShellRecoveryState {
    let Ok(state) = take_shell_state() else {
        return ShellRecoveryState::default();
    };
    let recovery_state = ShellRecoveryState::capture(&state);
    restore_shell_state(state);
    recovery_state
}

pub(super) fn reset_recovered_runtime_shell_state(
    recovery_state: ShellRecoveryState,
) -> RuntimeAccessResult<()> {
    mutate_shell_state(|state| {
        restore_recovered_shell_state(state, recovery_state);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn recovery_preserves_host_bridge_verification_before_zeroing_shell_state() {
        let mut state = ShellState::default();

        state.set_namespace_id(NamespaceId::new(/*value*/ 77));
        state.note_host_bridge_verified(super::super::super::HostBridgeRevision::CURRENT);

        let recovery_state = ShellRecoveryState::capture(&state);
        restore_recovered_shell_state(&mut state, recovery_state);

        assert_eq!(
            recovery_state.namespace_id,
            Some(NamespaceId::new(/*value*/ 77))
        );
        assert_eq!(
            recovery_state.host_bridge_state,
            super::super::super::HostBridgeState::Verified {
                revision: super::super::super::HostBridgeRevision::CURRENT,
            }
        );
        assert_eq!(state.namespace_id(), None);
        assert_eq!(
            state.host_bridge_state(),
            super::super::super::HostBridgeState::Verified {
                revision: super::super::super::HostBridgeRevision::CURRENT,
            }
        );
    }
}

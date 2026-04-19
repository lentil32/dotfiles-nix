use super::super::ENGINE_CONTEXT;
use super::super::EngineContext;
use super::super::EngineState;
use super::super::HostBridgeState;
use super::super::cursor::BufferMetadata;
use super::super::logging::set_log_level;
use super::super::logging::warn;
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
use super::EditorViewport;
use super::EngineAccessResult;
use super::IngressReadSnapshot;
use super::diagnostics::reset_transient_event_state;
use super::timers::now_ms;
use crate::config::RuntimeConfig;
use crate::core::effect::ProbePolicy;
use crate::core::state::CoreState;
use crate::core::state::CursorColorProbeWitness;
use crate::core::state::CursorColorSample;
use crate::core::state::CursorTextContext;
use crate::core::types::Generation;
use crate::draw::recover_all_namespaces;
use nvim_oxi::Object;
use nvim_oxi::Result;
use nvim_oxi::api;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::panic::resume_unwind;
use std::sync::Arc;

fn with_engine_state_access<R>(
    accessor: impl FnOnce(&mut EngineState) -> R,
) -> EngineAccessResult<R> {
    let mut state = ENGINE_CONTEXT.with(EngineContext::take_state)?;
    match catch_unwind(AssertUnwindSafe(|| accessor(&mut state))) {
        Ok(output) => {
            ENGINE_CONTEXT.with(|context| context.restore_state(state));
            Ok(output)
        }
        Err(panic_payload) => {
            let namespace_id = recover_engine_state(&mut state);
            ENGINE_CONTEXT.with(|context| context.restore_state(state));
            post_engine_state_recovery(namespace_id);
            resume_unwind(panic_payload);
        }
    }
}

pub(crate) fn read_engine_state<R>(
    reader: impl FnOnce(&EngineState) -> R,
) -> EngineAccessResult<R> {
    with_engine_state_access(|state| reader(state))
}

pub(crate) fn mutate_engine_state<R>(
    mutator: impl FnOnce(&mut EngineState) -> R,
) -> EngineAccessResult<R> {
    with_engine_state_access(mutator)
}

#[cfg(not(test))]
pub(crate) fn ingress_read_snapshot() -> EngineAccessResult<IngressReadSnapshot> {
    IngressReadSnapshot::capture()
}

pub(crate) fn ingress_read_snapshot_with_current_buffer(
    current_buffer: Option<&api::Buffer>,
) -> EngineAccessResult<IngressReadSnapshot> {
    IngressReadSnapshot::capture_with_current_buffer(current_buffer)
}

pub(crate) fn editor_viewport_for_bounds() -> Result<EditorViewport> {
    mutate_engine_state(|state| state.shell.editor_viewport_cache.read_for_bounds())
        .map_err(nvim_oxi::Error::from)?
}

pub(crate) fn editor_viewport_for_command_row() -> Result<EditorViewport> {
    mutate_engine_state(|state| state.shell.editor_viewport_cache.read_for_command_row())
        .map_err(nvim_oxi::Error::from)?
}

pub(crate) fn refresh_editor_viewport_cache() -> Result<()> {
    mutate_engine_state(|state| state.shell.editor_viewport_cache.refresh())
        .map_err(nvim_oxi::Error::from)?
}

pub(crate) fn buffer_text_revision(buffer_handle: i64) -> EngineAccessResult<Generation> {
    mutate_engine_state(|state| {
        state
            .shell
            .buffer_text_revision_cache
            .current(buffer_handle)
    })
}

pub(crate) fn resolved_current_buffer_event_policy(
    snapshot: &IngressReadSnapshot,
    buffer: &api::Buffer,
) -> Result<BufferEventPolicy> {
    let buffer_handle = i64::from(buffer.handle());
    let metadata = mutate_engine_state(|state| state.shell.buffer_metadata_cache.read(buffer))
        .map_err(nvim_oxi::Error::from)??;
    resolve_buffer_event_policy_for_metadata(snapshot, buffer_handle, &metadata, now_ms())
}

pub(crate) fn resolve_buffer_event_policy_for_metadata(
    snapshot: &IngressReadSnapshot,
    buffer_handle: i64,
    metadata: &BufferMetadata,
    observed_at_ms: f64,
) -> Result<BufferEventPolicy> {
    let (previous, telemetry) = read_engine_state(|state| {
        (
            state
                .shell
                .buffer_perf_policy_cache
                .cached_policy(buffer_handle),
            state
                .shell
                .buffer_perf_telemetry_cache
                .telemetry(buffer_handle)
                .unwrap_or_default(),
        )
    })
    .map_err(nvim_oxi::Error::from)?;
    let policy =
        buffer_event_policy_from_snapshot(snapshot, metadata, previous, telemetry, observed_at_ms);
    mutate_engine_state(|state| {
        state
            .shell
            .buffer_perf_policy_cache
            .store_policy(buffer_handle, policy);
    })
    .map_err(nvim_oxi::Error::from)?;
    Ok(policy)
}

#[cfg(test)]
pub(crate) fn core_state() -> EngineAccessResult<CoreState> {
    read_engine_state(EngineState::clone_core_state)
}

#[cfg(test)]
pub(crate) fn set_core_state(next_state: CoreState) -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state.set_core_state(next_state);
    })
}

pub(crate) fn cursor_color_colorscheme_generation() -> EngineAccessResult<Generation> {
    read_engine_state(|state| state.shell.probe_cache.colorscheme_generation())
}

pub(crate) fn cursor_color_cache_generation() -> EngineAccessResult<Generation> {
    read_engine_state(|state| state.shell.probe_cache.cursor_color_cache_generation())
}

pub(crate) fn cached_cursor_color_sample_for_probe(
    witness: &CursorColorProbeWitness,
    probe_policy: ProbePolicy,
    reuse: crate::core::state::ProbeReuse,
) -> EngineAccessResult<Option<CachedCursorColorProbeSample>> {
    mutate_engine_state(|state| {
        state
            .shell
            .probe_cache
            .cached_cursor_color_sample_for_probe(witness, probe_policy, reuse)
    })
}

pub(crate) fn store_cursor_color_sample(
    witness: CursorColorProbeWitness,
    sample: Option<CursorColorSample>,
) -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state
            .shell
            .probe_cache
            .store_cursor_color_sample(witness, sample);
    })
}

pub(crate) fn note_cursor_color_observation_boundary() -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state
            .shell
            .probe_cache
            .note_cursor_color_observation_boundary();
    })
}

pub(crate) fn cached_cursor_text_context(
    key: &CursorTextContextCacheKey,
) -> EngineAccessResult<CursorTextContextCacheLookup> {
    mutate_engine_state(|state| state.shell.probe_cache.cached_cursor_text_context(key))
}

pub(crate) fn store_cursor_text_context(
    key: CursorTextContextCacheKey,
    context: Option<CursorTextContext>,
) -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state
            .shell
            .probe_cache
            .store_cursor_text_context(key, context);
    })
}

pub(crate) fn cached_conceal_regions(
    key: &ConcealCacheKey,
) -> EngineAccessResult<ConcealCacheLookup> {
    mutate_engine_state(|state| state.shell.probe_cache.cached_conceal_regions(key))
}

pub(crate) fn note_conceal_read_boundary() -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state.shell.probe_cache.note_conceal_read_boundary();
    })
}

pub(crate) fn cached_conceal_delta(
    key: &ConcealDeltaCacheKey,
) -> EngineAccessResult<ConcealDeltaCacheLookup> {
    mutate_engine_state(|state| state.shell.probe_cache.cached_conceal_delta(key))
}

pub(crate) fn store_conceal_regions(
    key: ConcealCacheKey,
    scanned_to_col1: i64,
    regions: Arc<[ConcealRegion]>,
) -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state
            .shell
            .probe_cache
            .store_conceal_regions(key, scanned_to_col1, regions);
    })
}

pub(crate) fn store_conceal_delta(
    key: ConcealDeltaCacheKey,
    current_col1: i64,
    delta: i64,
) -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state
            .shell
            .probe_cache
            .store_conceal_delta(key, current_col1, delta);
    })
}

pub(crate) fn cached_conceal_screen_cell(
    key: &ConcealScreenCellCacheKey,
) -> EngineAccessResult<ConcealScreenCellCacheLookup> {
    mutate_engine_state(|state| state.shell.probe_cache.cached_conceal_screen_cell(key))
}

pub(crate) fn store_conceal_screen_cell(
    key: ConcealScreenCellCacheKey,
    cell: Option<ConcealScreenCell>,
) -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state.shell.probe_cache.store_conceal_screen_cell(key, cell);
    })
}

pub(crate) fn note_cursor_color_colorscheme_change() -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state.shell.note_cursor_color_colorscheme_change();
    })
}

pub(crate) fn take_background_probe_request_scratch() -> EngineAccessResult<Vec<Object>> {
    mutate_engine_state(|state| state.shell.take_background_probe_request_scratch())
}

pub(crate) fn reclaim_background_probe_request_scratch(
    scratch: Vec<Object>,
) -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state
            .shell
            .reclaim_background_probe_request_scratch(scratch);
    })
}

pub(crate) fn take_conceal_regions_scratch() -> EngineAccessResult<Vec<ConcealRegion>> {
    mutate_engine_state(|state| state.shell.take_conceal_regions_scratch())
}

pub(crate) fn reclaim_conceal_regions_scratch(
    scratch: Vec<ConcealRegion>,
) -> EngineAccessResult<()> {
    mutate_engine_state(|state| {
        state.shell.reclaim_conceal_regions_scratch(scratch);
    })
}

pub(crate) fn reset_core_state() {
    if let Err(err) = mutate_engine_state(|state| {
        let runtime = state.core_state_mut().take_runtime();
        state.set_core_state(CoreState::default().with_runtime(runtime));
    }) {
        warn(&format!(
            "engine state re-entered during core reset; keeping existing state: {err}"
        ));
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ShellRecoveryState {
    namespace_id: Option<u32>,
    host_bridge_state: HostBridgeState,
}

fn recover_engine_state(state: &mut EngineState) -> Option<u32> {
    let recovery_state = ShellRecoveryState {
        namespace_id: state.shell.namespace_id(),
        host_bridge_state: state.shell.host_bridge_state(),
    };
    *state = EngineState::default();
    state.shell.host_bridge_state = recovery_state.host_bridge_state;
    recovery_state.namespace_id
}

fn post_engine_state_recovery(namespace_id: Option<u32>) {
    set_log_level(RuntimeConfig::default().logging_level);
    warn("engine state panicked while borrowed; resetting runtime state");
    if let Some(namespace_id) = namespace_id {
        recover_all_namespaces(namespace_id);
    }
    reset_transient_event_state();
}

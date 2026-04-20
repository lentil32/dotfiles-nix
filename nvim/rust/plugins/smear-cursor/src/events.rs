use crate::core::state::CoreState;
use std::cell::Cell;
use std::cell::RefCell;
use thiserror::Error;

mod buffer_text_revision;
mod cursor;
mod event_loop;
mod handlers;
mod host_bridge;
mod ingress;
mod lifecycle;
mod logging;
mod lru_cache;
mod options;
mod policy;
pub(crate) mod probe_cache;
mod runtime;
mod surface;
mod timer_protocol;
mod timers;
mod trace;

use buffer_text_revision::BufferTextRevisionCache;
use cursor::BufferMetadataCache;
use nvim_oxi::Object;
use policy::BufferEventPolicyCache;
use policy::BufferPerfTelemetryCache;
use probe_cache::ConcealRegion;
use probe_cache::ProbeCacheState;
use runtime::CoreTimerHandles;
use runtime::EditorViewportCache;
use timer_protocol::HostCallbackId;

#[cfg(test)]
mod tests;

pub(crate) use handlers::on_autocmd_event;
pub(crate) use lifecycle::diagnostics;
pub(crate) use lifecycle::on_autocmd_payload_event;
pub(crate) use lifecycle::setup;
pub(crate) use lifecycle::toggle;
pub(crate) use lifecycle::validation_counters;
pub(crate) use logging::warn;
pub(crate) use runtime::editor_viewport_for_bounds;
pub(crate) use runtime::record_compiled_field_cache_hit;
pub(crate) use runtime::record_compiled_field_cache_miss;
pub(crate) use runtime::record_effect_failure;
pub(crate) use runtime::record_particle_aggregation;
pub(crate) use runtime::record_particle_overlay_refresh;
pub(crate) use runtime::record_particle_simulation_step;
pub(crate) use runtime::record_planner_candidate_cells_built_count;
pub(crate) use runtime::record_planner_candidate_query_cells_count;
pub(crate) use runtime::record_planner_compiled_cells_emitted_count;
pub(crate) use runtime::record_planner_compiled_query_cells_count;
pub(crate) use runtime::record_planner_local_query;
pub(crate) use runtime::record_planner_local_query_compile;
pub(crate) use runtime::record_planner_local_query_envelope_area_cells;
pub(crate) use runtime::record_planner_reference_compile;
pub(crate) use runtime::record_planning_preview_copied_particles;
#[cfg(feature = "perf-counters")]
pub(crate) use runtime::record_planning_preview_copy;
pub(crate) use runtime::record_planning_preview_invocation;
pub(crate) use runtime::record_projection_reuse_hit;
pub(crate) use runtime::record_projection_reuse_miss;
pub(crate) use timers::on_core_timer_fired_event;
pub(crate) use timers::schedule_guarded;

const LOG_SOURCE_NAME: &str = "smear_cursor";
const LOG_LEVEL_TRACE: i64 = 0;
const LOG_LEVEL_DEBUG: i64 = 1;
const LOG_LEVEL_WARN: i64 = 3;
const LOG_LEVEL_INFO: i64 = 2;
const LOG_LEVEL_ERROR: i64 = 4;
const AUTOCMD_GROUP_NAME: &str = "RsSmearCursor";
const CALLBACK_DURATION_EWMA_ALPHA: f64 = 0.25;

fn update_callback_duration_ewma(previous_estimate_ms: f64, duration_ms: f64) -> Option<f64> {
    if !duration_ms.is_finite() {
        return None;
    }

    let observed = duration_ms.max(0.0);
    Some(if previous_estimate_ms <= 0.0 {
        observed
    } else {
        previous_estimate_ms + CALLBACK_DURATION_EWMA_ALPHA * (observed - previous_estimate_ms)
    })
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct HostBridgeRevision(u32);

impl HostBridgeRevision {
    const CURRENT: Self = Self(15);

    const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
enum HostBridgeState {
    #[default]
    Unverified,
    Verified {
        revision: HostBridgeRevision,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum RealCursorVisibility {
    Hidden,
    Visible,
}

#[derive(Debug, Default)]
struct ShellState {
    // Shell-owned state is intentionally non-authoritative: caches, reusable
    // scratch buffers, host timer ids, telemetry, and host capability
    // witnesses live here.
    // snapshot: host capability and witness state retained across cache purges.
    namespace_id: Option<u32>,
    host_bridge_state: HostBridgeState,
    core_timer_handles: CoreTimerHandles,
    next_host_callback_id: u64,
    editor_viewport_cache: EditorViewportCache,
    buffer_metadata_cache: BufferMetadataCache,
    real_cursor_visibility: Option<RealCursorVisibility>,
    // cache: purgeable shell-local reuse state and scratch storage.
    probe_cache: ProbeCacheState,
    background_probe_request_scratch: Vec<Object>,
    conceal_regions_scratch: Vec<ConcealRegion>,
    buffer_text_revision_cache: BufferTextRevisionCache,
    buffer_perf_policy_cache: BufferEventPolicyCache,
    // telemetry: execution-cost and probe-pressure signals derived from shell work.
    buffer_perf_telemetry_cache: BufferPerfTelemetryCache,
}

impl ShellState {
    const fn namespace_id(&self) -> Option<u32> {
        self.namespace_id
    }

    fn set_namespace_id(&mut self, namespace_id: u32) {
        self.namespace_id = Some(namespace_id);
    }

    const fn host_bridge_state(&self) -> HostBridgeState {
        self.host_bridge_state
    }

    fn note_host_bridge_verified(&mut self, revision: HostBridgeRevision) {
        self.host_bridge_state = HostBridgeState::Verified { revision };
    }

    fn allocate_host_callback_id(&mut self) -> HostCallbackId {
        HostCallbackId::next(&mut self.next_host_callback_id)
    }

    fn note_cursor_color_colorscheme_change(&mut self) {
        self.probe_cache.note_cursor_color_colorscheme_change();
        self.clear_real_cursor_visibility();
    }

    fn invalidate_editor_viewport_cache(&mut self) {
        self.editor_viewport_cache.invalidate();
    }

    fn invalidate_buffer_metadata(&mut self, buffer_handle: i64) {
        self.buffer_metadata_cache.invalidate_buffer(buffer_handle);
        // Buffer event policy is derived from buffer-local metadata, so this
        // metadata boundary remains the single owner of policy invalidation.
        self.buffer_perf_policy_cache
            .invalidate_buffer(buffer_handle);
    }

    fn invalidate_conceal_probe_caches(&mut self, buffer_handle: i64) {
        self.probe_cache.invalidate_conceal_buffer(buffer_handle);
    }

    fn invalidate_buffer_local_probe_caches(&mut self, buffer_handle: i64) {
        self.probe_cache.invalidate_buffer(buffer_handle);
    }

    fn invalidate_buffer_local_caches(&mut self, buffer_handle: i64) {
        self.invalidate_buffer_metadata(buffer_handle);
        self.buffer_perf_telemetry_cache
            .invalidate_buffer(buffer_handle);
        self.invalidate_buffer_local_probe_caches(buffer_handle);
        self.buffer_text_revision_cache.clear_buffer(buffer_handle);
    }

    fn reset_transient_caches(&mut self) {
        self.probe_cache.reset();
        self.invalidate_editor_viewport_cache();
        self.buffer_metadata_cache.clear();
        self.buffer_perf_policy_cache.clear();
        self.buffer_perf_telemetry_cache.clear();
        self.buffer_text_revision_cache.clear();
        self.next_host_callback_id = 0;
        self.release_cleanup_cold_storage();
        self.clear_real_cursor_visibility();
    }

    fn take_background_probe_request_scratch(&mut self) -> Vec<Object> {
        std::mem::take(&mut self.background_probe_request_scratch)
    }

    fn reclaim_background_probe_request_scratch(&mut self, mut scratch: Vec<Object>) {
        scratch.clear();
        self.background_probe_request_scratch = scratch;
    }

    fn take_conceal_regions_scratch(&mut self) -> Vec<ConcealRegion> {
        std::mem::take(&mut self.conceal_regions_scratch)
    }

    fn reclaim_conceal_regions_scratch(&mut self, mut scratch: Vec<ConcealRegion>) {
        scratch.clear();
        self.conceal_regions_scratch = scratch;
    }

    fn release_cleanup_cold_storage(&mut self) {
        self.background_probe_request_scratch = Vec::new();
        self.conceal_regions_scratch = Vec::new();
    }

    #[cfg(test)]
    fn background_probe_request_scratch_capacity(&self) -> usize {
        self.background_probe_request_scratch.capacity()
    }

    #[cfg(test)]
    fn conceal_regions_scratch_capacity(&self) -> usize {
        self.conceal_regions_scratch.capacity()
    }

    const fn real_cursor_visibility(&self) -> Option<RealCursorVisibility> {
        self.real_cursor_visibility
    }

    fn note_real_cursor_visibility(&mut self, visibility: RealCursorVisibility) {
        self.real_cursor_visibility = Some(visibility);
    }

    fn clear_real_cursor_visibility(&mut self) {
        self.real_cursor_visibility = None;
    }
}

#[derive(Debug, Default)]
struct EngineState {
    // Engine state preserves one top-level split: reducer truth in `core_state`
    // and shell-owned cache/snapshot/telemetry state in `shell`.
    shell: ShellState,
    core_state: CoreState,
}

impl EngineState {
    fn core_state(&self) -> &CoreState {
        &self.core_state
    }

    fn core_state_mut(&mut self) -> &mut CoreState {
        &mut self.core_state
    }

    fn take_core_state(&mut self) -> CoreState {
        std::mem::take(&mut self.core_state)
    }

    #[cfg(test)]
    fn clone_core_state(&self) -> CoreState {
        self.core_state.clone()
    }

    fn set_core_state(&mut self, next_state: CoreState) {
        self.core_state = next_state;
    }
}

#[derive(Debug)]
enum EngineStateSlot {
    Ready(Box<EngineState>),
    InUse,
}

impl Default for EngineStateSlot {
    fn default() -> Self {
        Self::Ready(Box::default())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub(super) enum EngineAccessError {
    #[error("engine state re-entered while already in use")]
    Reentered,
}

impl From<EngineAccessError> for nvim_oxi::Error {
    fn from(error: EngineAccessError) -> Self {
        crate::other_error(error.to_string())
    }
}

#[derive(Debug)]
struct EngineContext {
    state: RefCell<EngineStateSlot>,
    log_level: Cell<i64>,
}

impl EngineContext {
    fn new() -> Self {
        Self {
            state: RefCell::new(EngineStateSlot::Ready(Box::default())),
            log_level: Cell::new(LOG_LEVEL_INFO),
        }
    }

    fn take_state(&self) -> Result<EngineState, EngineAccessError> {
        let mut slot = self.state.borrow_mut();
        match std::mem::replace(&mut *slot, EngineStateSlot::InUse) {
            EngineStateSlot::Ready(state) => Ok(*state),
            EngineStateSlot::InUse => Err(EngineAccessError::Reentered),
        }
    }

    fn restore_state(&self, state: EngineState) {
        let mut slot = self.state.borrow_mut();
        let previous = std::mem::replace(&mut *slot, EngineStateSlot::Ready(Box::new(state)));
        debug_assert!(matches!(previous, EngineStateSlot::InUse));
    }
}

thread_local! {
    // CONTEXT: smear_cursor funnels host callbacks back through Neovim's scheduled
    // main-thread path, so engine state only needs single-thread interior mutability.
    static ENGINE_CONTEXT: EngineContext = EngineContext::new();
}

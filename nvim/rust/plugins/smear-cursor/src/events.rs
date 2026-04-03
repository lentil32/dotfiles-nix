use crate::core::state::CoreState;
use std::cell::Cell;
use std::cell::RefCell;
use thiserror::Error;

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
mod timers;
mod trace;

use policy::BufferEventPolicyCache;
use policy::BufferPerfTelemetryCache;
use probe_cache::ProbeCacheState;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use cursor::ConcealScreenCellView;
pub(crate) use handlers::on_autocmd_event;
pub(crate) use lifecycle::diagnostics;
pub(crate) use lifecycle::setup;
pub(crate) use lifecycle::toggle;
pub(crate) use logging::warn;
pub(crate) use runtime::record_effect_failure;
pub(crate) use runtime::record_planner_candidate_cells_built_count;
pub(crate) use runtime::record_planner_candidate_query_cells_count;
pub(crate) use runtime::record_planner_compiled_cells_emitted_count;
pub(crate) use runtime::record_planner_compiled_query_cells_count;
pub(crate) use runtime::record_planner_local_query;
pub(crate) use runtime::record_planner_local_query_compile;
pub(crate) use runtime::record_planner_local_query_envelope_area_cells;
pub(crate) use runtime::record_planner_reference_compile;
pub(crate) use timers::on_core_timer_event;
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
    const CURRENT: Self = Self(7);

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

#[derive(Debug, Default)]
struct ShellState {
    namespace_id: Option<u32>,
    host_bridge_state: HostBridgeState,
    probe_cache: ProbeCacheState,
    buffer_perf_policy_cache: BufferEventPolicyCache,
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
}

#[derive(Debug, Default)]
struct EngineState {
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

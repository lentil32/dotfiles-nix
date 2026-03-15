use crate::core::state::{CoreState, CursorColorProbeWitness, CursorColorSample};
use crate::core::types::Generation;
use std::cell::{Cell, RefCell};
use thiserror::Error;

mod cursor;
mod event_loop;
mod handlers;
mod host_bridge;
mod ingress;
mod lifecycle;
mod logging;
mod options;
mod policy;
mod probe_cache;
mod runtime;
mod timers;
mod trace;

use probe_cache::{CursorColorCacheLookup, ProbeCacheState};

#[cfg(test)]
mod tests;

pub(crate) use handlers::on_autocmd_event;
pub(crate) use lifecycle::{diagnostics, setup, toggle};
pub(crate) use logging::warn;
pub(crate) use runtime::record_effect_failure;
pub(crate) use timers::on_core_timer_event;
pub(crate) use timers::schedule_guarded;

const LOG_SOURCE_NAME: &str = "smear_cursor";
const LOG_LEVEL_TRACE: i64 = 0;
const LOG_LEVEL_DEBUG: i64 = 1;
const LOG_LEVEL_WARN: i64 = 3;
const LOG_LEVEL_INFO: i64 = 2;
const LOG_LEVEL_ERROR: i64 = 4;
const AUTOCMD_GROUP_NAME: &str = "RsSmearCursor";
const CURSOR_COLOR_LUAEVAL_EXPR: &str = r##"(function()
  local function get_hl_color(group, attr)
    local hl = vim.api.nvim_get_hl(0, { name = group, link = false })
    if hl[attr] then
      return string.format("#%06x", hl[attr])
    end
    return nil
  end

  local line = vim.fn.line(".")
  local col = vim.fn.col(".")

  -- Fast path: resolve the effective syntax group at the cursor position.
  local syn_id = vim.fn.synID(line, col, 1)
  if type(syn_id) == "number" and syn_id > 0 then
    local trans_id = vim.fn.synIDtrans(syn_id)
    local syn_color = vim.fn.synIDattr(trans_id, "fg#")
    if type(syn_color) == "string" and syn_color ~= "" then
      return syn_color
    end

    local syn_group = vim.fn.synIDattr(trans_id, "name")
    if type(syn_group) == "string" and syn_group ~= "" then
      local color = get_hl_color(syn_group, "fg")
      if color then
        return color
      end
    end
  end

  local cursor = { line - 1, col - 1 }

  if vim.bo.buftype == "" and vim.b.ts_highlight then
    local ok, captures = pcall(vim.treesitter.get_captures_at_pos, 0, cursor[1], cursor[2])
    if ok and type(captures) == "table" then
      local ts_hl_group
      for _, capture in pairs(captures) do
        ts_hl_group = "@" .. capture.capture .. "." .. capture.lang
      end
      if ts_hl_group then
        local color = get_hl_color(ts_hl_group, "fg")
        if color then
          return color
        end
      end
    end
  end

  if vim.bo.buftype ~= "" and vim.bo.buftype ~= "acwrite" then
    return nil
  end

  local extmarks = vim.api.nvim_buf_get_extmarks(
    0,
    -1,
    cursor,
    cursor,
    { details = true, overlap = true, limit = 32 }
  )
  for _, extmark in ipairs(extmarks) do
    local details = extmark[4]
    local hl_group = details and details.hl_group
    if hl_group then
      local color = get_hl_color(hl_group, "fg")
      if color then
        return color
      end
    end
  end

  return nil
end)()"##;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct HostBridgeRevision(u32);

impl HostBridgeRevision {
    const CURRENT: Self = Self(2);

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

    fn cursor_color_colorscheme_generation(&self) -> Generation {
        self.probe_cache.colorscheme_generation()
    }

    fn cached_cursor_color_sample(
        &self,
        witness: &CursorColorProbeWitness,
    ) -> CursorColorCacheLookup {
        self.probe_cache.cached_cursor_color_sample(witness)
    }

    fn store_cursor_color_sample(
        &mut self,
        witness: CursorColorProbeWitness,
        sample: Option<CursorColorSample>,
    ) {
        self.probe_cache.store_cursor_color_sample(witness, sample);
    }

    fn note_cursor_color_colorscheme_change(&mut self) {
        self.probe_cache.note_cursor_color_colorscheme_change();
    }

    fn reset_probe_caches(&mut self) {
        self.probe_cache.reset();
    }
}

#[derive(Debug, Default)]
struct EngineState {
    shell: ShellState,
    core_state: CoreState,
}

impl EngineState {
    fn core_state(&self) -> CoreState {
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
        nvim_oxi::api::Error::Other(error.to_string()).into()
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

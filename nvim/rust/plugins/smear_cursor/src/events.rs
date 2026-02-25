use crate::state::RuntimeState;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::AtomicI64;
use std::sync::{LazyLock, Mutex};

mod cursor;
mod event_loop;
mod geometry;
mod handlers;
mod lifecycle;
mod logging;
mod options;
mod policy;
mod runtime;
mod timers;

#[cfg(test)]
mod tests;

pub(crate) use handlers::on_key_event;
pub(crate) use lifecycle::{setup, toggle};

const LOG_SOURCE_NAME: &str = "smear_cursor";
const LOG_LEVEL_TRACE: i64 = 0;
const LOG_LEVEL_DEBUG: i64 = 1;
const LOG_LEVEL_WARN: i64 = 3;
const LOG_LEVEL_INFO: i64 = 2;
const LOG_LEVEL_ERROR: i64 = 4;
const AUTOCMD_GROUP_NAME: &str = "RsSmearCursor";
const MIN_RENDER_CLEANUP_DELAY_MS: u64 = 200;
const MIN_RENDER_HARD_PURGE_DELAY_MS: u64 = 3_000;
const RENDER_HARD_PURGE_DELAY_MULTIPLIER: u64 = 8;
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

#[derive(Debug, Clone, Copy, Default)]
struct RenderCleanupGeneration {
    value: u64,
}

impl RenderCleanupGeneration {
    fn bump(&mut self) -> u64 {
        self.value = self.value.wrapping_add(1);
        self.value
    }

    const fn current(self) -> u64 {
        self.value
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RenderGeneration {
    value: u64,
}

impl RenderGeneration {
    fn bump(&mut self) -> u64 {
        self.value = self.value.wrapping_add(1);
        self.value
    }

    const fn current(self) -> u64 {
        self.value
    }
}

#[derive(Debug, Default)]
struct EngineState {
    runtime: RuntimeState,
    render_cleanup_generation: RenderCleanupGeneration,
    render_generation: RenderGeneration,
}

impl EngineState {
    fn bump_render_cleanup_generation(&mut self) -> u64 {
        self.render_cleanup_generation.bump()
    }

    const fn current_render_cleanup_generation(&self) -> u64 {
        self.render_cleanup_generation.current()
    }

    fn bump_render_generation(&mut self) -> u64 {
        self.render_generation.bump()
    }

    const fn current_render_generation(&self) -> u64 {
        self.render_generation.current()
    }
}

struct RuntimeStateGuard(std::sync::MutexGuard<'static, EngineState>);

impl Deref for RuntimeStateGuard {
    type Target = RuntimeState;

    fn deref(&self) -> &Self::Target {
        &self.0.runtime
    }
}

impl DerefMut for RuntimeStateGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0.runtime
    }
}

#[derive(Debug)]
struct EngineContext {
    state: Mutex<EngineState>,
    log_level: AtomicI64,
}

impl EngineContext {
    fn new() -> Self {
        Self {
            state: Mutex::new(EngineState::default()),
            log_level: AtomicI64::new(LOG_LEVEL_INFO),
        }
    }
}

static ENGINE_CONTEXT: LazyLock<EngineContext> = LazyLock::new(EngineContext::new);

local rust
local load_error
local notified = false

local function try_load()
  if rust then
    return true
  end
  local ok, mod = pcall(require, "project_root")
  if ok then
    rust = mod
    return true
  end
  load_error = mod
  if vim.g.project_root_debug and not notified then
    notified = true
    vim.notify("project_root require failed: " .. tostring(load_error), vim.log.levels.ERROR)
  end
  return false
end

if not try_load() then
  local M = {}

  function M.setup(_config)
    if try_load() then
      rust.setup(_config)
    end
  end

  function M.swap_root(_buf)
    if try_load() then
      rust.swap_root(_buf)
    end
  end

  function M.project_root()
    if try_load() then
      return rust.project_root()
    end
    return nil
  end

  function M.show_project_root()
    if try_load() then
      rust.show_project_root()
      return
    end
    vim.notify("project_root plugin not loaded", vim.log.levels.WARN)
  end

  return M
end

local M = {}

function M.setup(config)
  rust.setup(config)
end

function M.swap_root(buf)
  rust.swap_root(buf)
end

function M.project_root()
  return rust.project_root()
end

function M.show_project_root()
  rust.show_project_root()
end

return M

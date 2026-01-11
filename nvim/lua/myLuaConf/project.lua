local ok, rust = pcall(require, "project_root")
if not ok then
  local M = {}

  function M.setup(_config) end

  function M.swap_root(_buf) end

  function M.project_root()
    return nil
  end

  function M.show_project_root()
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

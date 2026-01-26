---@class ProjectRootConfig
---@field root_indicators? string[] List of files/dirs that mark the project root.

---@class ProjectRootApi
---@field setup fun(config?: ProjectRootConfig)
---@field swap_root fun(buf?: integer|nil)
---@field project_root fun(): string|nil
---@field project_root_or_warn fun(): string|nil
---@field show_project_root fun()

---@type ProjectRootApi
local rust = require("project_root")

local M = {}

---@param config? ProjectRootConfig
function M.setup(config)
  rust.setup(config)
end

---@param buf? integer
function M.swap_root(buf)
  rust.swap_root(buf)
end

---@return string|nil
function M.project_root()
  return rust.project_root()
end

---@return string|nil
function M.project_root_or_warn()
  return rust.project_root_or_warn()
end

function M.show_project_root()
  rust.show_project_root()
end

return M

---@meta

---@class rs_project_root.Config
---@field root_indicators? string[]

local M = {}

---@param config? rs_project_root.Config
function M.setup(config) end

---@param buf? integer
function M.swap_root(buf) end

---@return string|nil
function M.project_root() end

---@return string|nil
function M.project_root_or_warn() end

function M.show_project_root() end

return M

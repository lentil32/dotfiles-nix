local rust = require("project_root")

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

function M.project_root_or_warn()
  return rust.project_root_or_warn()
end

function M.show_project_root()
  rust.show_project_root()
end

return M

local util = require("config.util")

local M = {}

local function resolve_project_root()
  local root
  local project = util.try_require("project")
  local project_api = util.try_require("project.api")
  local get_root = project_api and project_api.get_project_root or (project and project.get_project_root)
  if get_root then
    local buf = vim.api.nvim_get_current_buf()
    if vim.bo[buf].buftype ~= "terminal" then
      root = get_root(buf)
    else
      local alt = vim.fn.bufnr("#")
      if alt > 0 and vim.bo[alt].buftype ~= "terminal" then
        root = get_root(alt)
      end
    end
  end
  if not root or root == "" then
    root = vim.fn.getcwd()
  end
  return root
end

function M.project_root()
  return resolve_project_root()
end

function M.show_project_root()
  local root = resolve_project_root()
  root = vim.fn.fnamemodify(root, ":~")
  vim.notify(root, vim.log.levels.INFO, { title = "Project root" })
end

return M

local project_api = require("project.api")

local M = {}

local function resolve_project_root()
  local root
  local buf = vim.api.nvim_get_current_buf()
  if vim.bo[buf].buftype == "terminal" then
    local alt = vim.fn.bufnr("#")
    if alt > 0 and vim.bo[alt].buftype ~= "terminal" then
      root = project_api.get_project_root(alt)
    end
  else
    root = project_api.get_project_root(buf)
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

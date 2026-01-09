local util = require("myLuaConf.util")

local M = {}

---@type Project.API|nil
local project_api = util.try_require("project.api")
---@type Project.Config|nil
local project_config = util.try_require("project.config")

function M.setup()
  local project = util.try_require("project")
  if not project then
    return
  end
  project.setup({
    use_lsp = true,
    patterns = { ".git", "package.json", "Cargo.toml", "flake.nix", "Makefile" },
    allow_different_owners = true,
    silent_chdir = true,
    show_hidden = true,
  })
end

local function is_disabled(buf)
  local api = project_api
  local config = project_config
  if not api or not config then
    return false
  end
  if not api.valid_bt(buf) then
    return true
  end
  local ft = util.get_buf_opt(buf, "filetype", "")
  local disabled = config.options and config.options.disable_on
  local list = disabled and disabled.ft or {}
  return vim.list_contains(list, ft)
end

local function resolve_project_root()
  local api = project_api
  if not api then
    return vim.fn.getcwd()
  end
  local buf = vim.api.nvim_get_current_buf()
  if is_disabled(buf) then
    local alt = vim.fn.bufnr("#")
    if alt > 0 and not is_disabled(alt) then
      buf = alt
    end
  end
  local root = api.get_project_root(buf)
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

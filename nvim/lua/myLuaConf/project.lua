local util = require("myLuaConf.util")

local M = {}

local patterns = { ".git", "package.json", "Cargo.toml", "flake.nix", "Makefile" }

---@type { valid_bt: fun(bufnr?: integer): boolean, get_project_root: fun(bufnr?: integer): string|nil, set_pwd: fun(dir?: string, method?: string): boolean|nil }|nil
local project_api = util.try_require("project.api")
---@type { options: Project.Config.Options }|nil
local project_config = util.try_require("project.config")

function M.setup()
  local project = util.try_require("project")
  if not project then
    return
  end
  project.setup({
    use_lsp = true,
    patterns = patterns,
    manual_mode = true,
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

local function buffer_path(buf)
  if not vim.api.nvim_buf_is_valid(buf) then
    return nil
  end
  local bt = vim.api.nvim_get_option_value("buftype", { buf = buf })
  if bt ~= "" then
    return nil
  end
  local name = vim.api.nvim_buf_get_name(buf)
  if name == "" then
    return nil
  end
  if vim.uri_from_bufnr then
    local uri = vim.uri_from_bufnr(buf)
    if uri and uri ~= "" and uri:sub(1, 7) ~= "file://" then
      return nil
    end
  elseif name:match("^[%a][%w+.-]*://") then
    return nil
  end
  return name
end

local function buffer_is_file(buf)
  local name = buffer_path(buf)
  if not name then
    return false
  end
  local snacks_util = util.try_require("snacks.util")
  if snacks_util and snacks_util.path_type then
    return snacks_util.path_type(name) ~= "directory"
  end
  return vim.fn.isdirectory(name) ~= 1
end

local function root_from_project(buf)
  local api = project_api
  if not api or is_disabled(buf) then
    return nil
  end
  local ok, root = pcall(api.get_project_root, buf)
  if ok and root and root ~= "" then
    return root
  end
end

local function resolve_project_root()
  local buf = vim.api.nvim_get_current_buf()
  if not buffer_is_file(buf) or is_disabled(buf) then
    local alt = vim.fn.bufnr("#")
    if buffer_is_file(alt) and not is_disabled(alt) then
      buf = alt
    else
      return nil
    end
  end
  return root_from_project(buf)
end

function M.project_root()
  return resolve_project_root()
end

function M.show_project_root()
  local root = resolve_project_root()
  if not root or root == "" then
    vim.notify("No project root found", vim.log.levels.WARN, { title = "Project root" })
    return
  end
  root = vim.fn.fnamemodify(root, ":~")
  vim.notify(root, vim.log.levels.INFO, { title = "Project root" })
end

return M

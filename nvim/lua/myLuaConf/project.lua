local util = require("myLuaConf.util")

local M = {}

local augroup = vim.api.nvim_create_augroup
local autocmd = vim.api.nvim_create_autocmd

---@class ProjectRoot.Config
---@field root_indicators? string[]

---@type string[]
local default_root_indicators = { ".git", "package.json", "Cargo.toml", "flake.nix", "Makefile" }

---@type string[]
local root_indicators = default_root_indicators
---@type boolean
local did_setup = false
---@type string
local root_var = "project_root"

---@param buf integer
---@return string|nil
local function get_buf_root(buf)
  if not (buf and vim.api.nvim_buf_is_valid(buf)) then
    return nil
  end
  return util.get_var(buf, root_var)
end

---@param buf integer
---@param root string|nil
local function set_buf_root(buf, root)
  if not (buf and vim.api.nvim_buf_is_valid(buf)) then
    return
  end
  vim.b[buf][root_var] = root
end

---@param buf integer
---@return string|nil
local function get_path_from_buffer(buf)
  if not (buf and vim.api.nvim_buf_is_valid(buf)) then
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
  if name:match("^oil://") then
    local ok, oil = pcall(require, "oil")
    if ok and oil.get_current_dir then
      return oil.get_current_dir()
    end
    return (name:gsub("^oil://", ""))
  end
  return name
end

---@param path string
---@return string|nil
local function root_from_path(path)
  if not path or path == "" then
    return nil
  end
  local start = path
  if not util.is_dir(start) then
    start = vim.fs.dirname(start)
  end
  local found = vim.fs.find(root_indicators, { path = start, upward = true })[1]
  if not found or found == "" then
    return nil
  end
  local root = vim.fs.dirname(found)
  if root == "" then
    return nil
  end
  return root
end

---@param buf integer
---@return string|nil
local function set_root_for_buffer(buf)
  local existing = get_buf_root(buf)
  if existing and existing ~= "" then
    return existing
  end
  local path = get_path_from_buffer(buf)
  if not path then
    return nil
  end
  local root = root_from_path(path)
  if root and root ~= "" then
    set_buf_root(buf, root)
  end
  return root
end

---@return string|nil
local function resolve_project_root()
  local buf = vim.api.nvim_get_current_buf()
  local root = set_root_for_buffer(buf)
  if root and root ~= "" then
    return root
  end
  local alt = vim.fn.bufnr("#")
  if alt and alt ~= -1 then
    local alt_root = set_root_for_buffer(alt)
    if alt_root and alt_root ~= "" then
      return alt_root
    end
  end
  return nil
end

---@param buf? integer
function M.swap_root(buf)
  set_root_for_buffer(buf or vim.api.nvim_get_current_buf())
end

local function setup_autocmd()
  local group = augroup("ProjectRoot", { clear = true })
  autocmd({ "BufEnter", "BufWinEnter", "BufFilePost" }, {
    group = group,
    callback = function(args)
      M.swap_root(args.buf)
    end,
  })
end

---@param config? ProjectRoot.Config
function M.setup(config)
  config = config or {}
  local indicators = config.root_indicators
  if indicators == nil then
    indicators = default_root_indicators
  end
  root_indicators = indicators
  if did_setup then
    return
  end
  setup_autocmd()
  did_setup = true
end

---@return string|nil
function M.project_root()
  return resolve_project_root()
end

---@return nil
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

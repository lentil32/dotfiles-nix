local util = require("myLuaConf.util")

local M = {}

local augroup = vim.api.nvim_create_augroup
local autocmd = vim.api.nvim_create_autocmd

---@class ProjectRoot.Config
---@field root_indicators? string[]

---@param path string|nil
---@return string|nil
local function normalize_path(path)
  if not path or path == "" then
    return nil
  end
  if vim.fs and vim.fs.normalize then
    return vim.fs.normalize(path)
  end
  return path
end

---@type string[]
local default_root_indicators = { ".git", "package.json", "Cargo.toml", "flake.nix", "Makefile" }

---@type string[]
local root_indicators = default_root_indicators
---@type boolean
local did_setup = false
---@type string
local root_var = "project_root"
---@type string
local root_for_var = "project_root_for"

---@param buf integer
---@return string|nil
local function get_buf_root(buf)
  if not (buf and vim.api.nvim_buf_is_valid(buf)) then
    return nil
  end
  local root = util.get_var(buf, root_var)
  if type(root) ~= "string" then
    return nil
  end
  return root
end

---@param buf integer
---@return string|nil
local function get_buf_root_for(buf)
  if not (buf and vim.api.nvim_buf_is_valid(buf)) then
    return nil
  end
  local path = util.get_var(buf, root_for_var)
  if type(path) ~= "string" then
    return nil
  end
  return path
end

---@param buf integer
---@param root string|nil
---@param path string|nil
local function set_buf_root(buf, root, path)
  if not (buf and vim.api.nvim_buf_is_valid(buf)) then
    return
  end
  vim.b[buf][root_var] = root or false
  vim.b[buf][root_for_var] = path or false
end

---@param buf integer
---@return string|nil
local function get_path_from_buffer(buf)
  if not (buf and vim.api.nvim_buf_is_valid(buf)) then
    return nil
  end
  local name = vim.api.nvim_buf_get_name(buf)
  if name == "" then
    return nil
  end
  if name:match("^oil://") then
    local ok, oil = pcall(require, "oil")
    if ok and oil.get_current_dir then
      return normalize_path(oil.get_current_dir())
    end
    return normalize_path((name:gsub("^oil://", "")))
  end
  local bt = vim.api.nvim_get_option_value("buftype", { buf = buf })
  if bt ~= "" then
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
  return normalize_path(name)
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
local function refresh_root_for_buffer(buf)
  local path = get_path_from_buffer(buf)
  if not path then
    set_buf_root(buf, nil, nil)
    return nil
  end
  local root = root_from_path(path)
  set_buf_root(buf, root, path)
  return root
end

---@param buf integer
---@param path string|nil
---@return string|nil
local function get_cached_root(buf, path)
  if not path then
    return nil
  end
  local cached_for = get_buf_root_for(buf)
  if cached_for ~= path then
    return nil
  end
  return get_buf_root(buf)
end

---@return string|nil
local function get_project_root()
  local buf = vim.api.nvim_get_current_buf()
  local path = get_path_from_buffer(buf)
  local root = get_cached_root(buf, path)
  if root and root ~= "" then
    return root
  end
  local alt = vim.fn.bufnr("#")
  if alt and alt ~= -1 then
    local alt_path = get_path_from_buffer(alt)
    local alt_root = get_cached_root(alt, alt_path)
    if alt_root and alt_root ~= "" then
      return alt_root
    end
  end
  return nil
end

---@param buf? integer
function M.swap_root(buf)
  refresh_root_for_buffer(buf or vim.api.nvim_get_current_buf())
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
  return get_project_root()
end

---@return nil
function M.show_project_root()
  local root = get_project_root()
  if not root or root == "" then
    vim.notify("No project root found", vim.log.levels.WARN, { title = "Project root" })
    return
  end
  root = vim.fn.fnamemodify(root, ":~")
  vim.notify(root, vim.log.levels.INFO, { title = "Project root" })
end

return M

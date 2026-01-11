local util = require("myLuaConf.util")

local M = {}

local patterns = { ".git", "package.json", "Cargo.toml", "flake.nix", "Makefile" }

function M.setup() end

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

local function root_from_path(path)
  if not path or path == "" then
    return nil
  end
  local start = path
  if not util.is_dir(start) then
    start = vim.fs.dirname(start)
  end
  local found = vim.fs.find(patterns, { path = start, upward = true })[1]
  if not found or found == "" then
    return nil
  end
  local root = vim.fs.dirname(found)
  if root == "" then
    return nil
  end
  return root
end

local function resolve_project_root()
  local buf = vim.api.nvim_get_current_buf()
  local path = buffer_path(buf)
  if not path then
    local alt = vim.fn.bufnr("#")
    path = buffer_path(alt)
  end
  if not path then
    return nil
  end
  return root_from_path(path)
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

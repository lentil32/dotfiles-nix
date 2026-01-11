local M = {}

function M.try_require(mod)
  local ok, ret = pcall(require, mod)
  if ok then
    return ret
  end
  return nil
end

local cached_snacks_util

local function snacks_util()
  if cached_snacks_util == false then
    return nil
  end
  if cached_snacks_util then
    return cached_snacks_util
  end
  cached_snacks_util = M.try_require("snacks.util") or false
  return cached_snacks_util or nil
end

function M.is_dir(path)
  if not path or path == "" then
    return false
  end
  local util = snacks_util()
  if util and util.path_type then
    return util.path_type(path) == "directory"
  end
  return vim.fn.isdirectory(path) == 1
end

function M.set_buf_opts(buf, opts)
  if not (buf and vim.api.nvim_buf_is_valid(buf)) then
    return
  end
  opts = opts or {}
  local util = snacks_util()
  if util and util.bo then
    util.bo(buf, opts)
    return
  end
  for k, v in pairs(opts) do
    vim.api.nvim_set_option_value(k, v, { buf = buf })
  end
end

function M.set_win_opts(win, opts)
  if not (win and win ~= -1 and vim.api.nvim_win_is_valid(win)) then
    return
  end
  opts = opts or {}
  local util = snacks_util()
  if util and util.wo then
    util.wo(win, opts)
    return
  end
  for k, v in pairs(opts) do
    vim.api.nvim_set_option_value(k, v, { scope = "local", win = win })
  end
end

function M.get_buf_opt(buf, opt, default)
  if not (buf and vim.api.nvim_buf_is_valid(buf)) then
    return default
  end
  local ok, ret = pcall(vim.api.nvim_get_option_value, opt, { buf = buf })
  if ok then
    return ret
  end
  return default
end

function M.get_win_opt(win, opt, default)
  if not (win and vim.api.nvim_win_is_valid(win)) then
    return default
  end
  local ok, ret = pcall(vim.api.nvim_get_option_value, opt, { win = win })
  if ok then
    return ret
  end
  return default
end

---@generic T
---@param buf? number
---@param name string
---@param default? T
---@return T
function M.get_var(buf, name, default)
  local util = snacks_util()
  if util and util.var then
    return util.var(buf, name, default)
  end
  local ok, ret = pcall(function()
    return vim.b[buf or 0][name]
  end)
  if ok and ret ~= nil then
    return ret
  end
  ret = vim.g[name]
  if ret ~= nil then
    return ret
  end
  return default
end

function M.edit_path(path)
  if not path or path == "" then
    return
  end
  vim.cmd("edit " .. vim.fn.fnameescape(path))
end

---@param wins integer[]
---@param cur integer
---@return integer|nil
local function next_window(wins, cur)
  local count = #wins
  if count <= 1 then
    return nil
  end
  for i = 1, count do
    if wins[i] == cur then
      return wins[(i % count) + 1]
    end
  end
  return wins[1]
end

---@return integer|nil
function M.other_window()
  local wins = vim.api.nvim_tabpage_list_wins(0)
  local cur = vim.api.nvim_get_current_win()
  return next_window(wins, cur)
end

---@return integer win
---@return boolean created
function M.get_or_create_other_window()
  local wins = vim.api.nvim_tabpage_list_wins(0)
  local cur = vim.api.nvim_get_current_win()
  local win = next_window(wins, cur)
  if win and vim.api.nvim_win_is_valid(win) then
    return win, false
  end
  vim.cmd("vsplit")
  local new_win = vim.api.nvim_get_current_win()
  if vim.api.nvim_win_is_valid(cur) then
    vim.api.nvim_set_current_win(cur)
  end
  return new_win, true
end

return M

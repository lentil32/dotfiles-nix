local M = {}

function M.try_require(mod)
  local ok, ret = pcall(require, mod)
  if ok then
    return ret
  end
  return nil
end

local function snacks_util()
  return M.try_require("snacks.util")
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
  local util = snacks_util()
  if util and util.bo then
    util.bo(buf, opts)
    return
  end
  for k, v in pairs(opts or {}) do
    vim.api.nvim_set_option_value(k, v, { buf = buf })
  end
end

function M.set_win_opts(win, opts)
  if not win or win == -1 then
    return
  end
  local util = snacks_util()
  if util and util.wo then
    util.wo(win, opts)
    return
  end
  for k, v in pairs(opts or {}) do
    vim.api.nvim_set_option_value(k, v, { scope = "local", win = win })
  end
end

function M.edit_path(path)
  if not path or path == "" then
    return
  end
  vim.cmd("edit " .. vim.fn.fnameescape(path))
end

return M

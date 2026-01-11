local M = {}

function M.try_require(mod)
  local ok, ret = pcall(require, mod)
  if ok then
    return ret
  end
  return nil
end

local rust = require("my_util")

function M.is_dir(path)
  return rust.is_dir(path)
end

function M.set_buf_opts(buf, opts)
  rust.set_buf_opts(buf, opts or {})
end

function M.set_win_opts(win, opts)
  rust.set_win_opts(win, opts or {})
end

function M.get_buf_opt(buf, opt, default)
  return rust.get_buf_opt(buf, opt, default)
end

function M.get_win_opt(win, opt, default)
  return rust.get_win_opt(win, opt, default)
end

---@generic T
---@param buf? number
---@param name string
---@param default? T
---@return T
function M.get_var(buf, name, default)
  return rust.get_var(buf, name, default)
end

function M.edit_path(path)
  rust.edit_path(path)
end

function M.other_window()
  return rust.other_window()
end

function M.get_or_create_other_window()
  return rust.get_or_create_other_window()
end

return M

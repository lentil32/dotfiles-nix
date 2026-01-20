local M = {}

local rust = nil
do
  local ok, mod = pcall(require, "my_readline")
  if ok then
    rust = mod
  end
end

local function feedkeys(keys)
  vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes(keys, true, false, true), "n", false)
end

local function is_insert_mode()
  return vim.fn.mode():sub(1, 1) == "i"
end

local function transpose_chars()
  local pos = vim.api.nvim_win_get_cursor(0)
  local row = pos[1]
  local col = pos[2]
  local line = vim.api.nvim_get_current_line()
  local len = #line

  if len < 2 or col == 0 then
    return
  end

  if col >= len then
    local before = line:sub(1, len - 2)
    local a = line:sub(len - 1, len - 1)
    local b = line:sub(len, len)
    vim.api.nvim_set_current_line(before .. b .. a)
    vim.api.nvim_win_set_cursor(0, { row, len })
    return
  end

  local before = line:sub(1, col - 1)
  local a = line:sub(col, col)
  local b = line:sub(col + 1, col + 1)
  local after = line:sub(col + 2)
  vim.api.nvim_set_current_line(before .. b .. a .. after)
  vim.api.nvim_win_set_cursor(0, { row, col + 1 })
end

function M.beginning_of_line()
  if rust and rust.beginning_of_line then
    rust.beginning_of_line()
    return
  end
  if is_insert_mode() then
    feedkeys("<C-o>0")
  end
end

function M.end_of_line()
  if rust and rust.end_of_line then
    rust.end_of_line()
    return
  end
  if is_insert_mode() then
    feedkeys("<C-o>$")
  end
end

function M.forward_word()
  if rust and rust.forward_word then
    rust.forward_word()
    return
  end
  if is_insert_mode() then
    feedkeys("<C-o>w")
  end
end

function M.backward_word()
  if rust and rust.backward_word then
    rust.backward_word()
    return
  end
  if is_insert_mode() then
    feedkeys("<C-o>b")
  end
end

function M.kill_word()
  if rust and rust.kill_word then
    rust.kill_word()
    return
  end
  if is_insert_mode() then
    feedkeys("<C-o>dw")
  end
end

function M.transpose_chars()
  if rust and rust.transpose_chars then
    rust.transpose_chars()
    return
  end
  if is_insert_mode() then
    transpose_chars()
  end
end

return M

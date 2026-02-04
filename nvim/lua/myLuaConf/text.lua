local rust = require("my_text")

local M = {}

local function visual_line_range()
  local mode = vim.fn.mode()
  if mode ~= "v" and mode ~= "V" and mode ~= "\22" then
    return nil
  end
  local start_line = vim.fn.getpos("'<")[2]
  local end_line = vim.fn.getpos("'>")[2]
  if start_line == 0 or end_line == 0 then
    return nil
  end
  return start_line, end_line
end

local function with_range(fn)
  return function()
    local start_line, end_line = visual_line_range()
    if start_line and end_line then
      fn(start_line, end_line)
      return
    end
    fn(nil, nil)
  end
end

M.sort_lines = with_range(rust.sort_lines)
M.sort_lines_reverse = with_range(rust.sort_lines_reverse)
M.sort_lines_by_column = with_range(rust.sort_lines_by_column)
M.sort_lines_by_column_reverse = with_range(rust.sort_lines_by_column_reverse)
M.randomize_lines = with_range(rust.randomize_lines)
M.uniquify_lines = with_range(rust.uniquify_lines)
M.duplicate_line_or_region = with_range(rust.duplicate_line_or_region)
M.kill_back_to_indentation = with_range(rust.kill_back_to_indentation)

return M

local M = {}

local function get_hl_color(group, attr)
  local hl = vim.api.nvim_get_hl(0, { name = group, link = false })
  if hl[attr] then
    return string.format("#%06x", hl[attr])
  end
  return nil
end

function M.cursor_color_at_cursor()
  local line = vim.fn.line(".")
  local col = vim.fn.col(".")

  local syn_id = vim.fn.synID(line, col, 1)
  if type(syn_id) == "number" and syn_id > 0 then
    local trans_id = vim.fn.synIDtrans(syn_id)
    local syn_color = vim.fn.synIDattr(trans_id, "fg#")
    if type(syn_color) == "string" and syn_color ~= "" then
      return syn_color
    end

    local syn_group = vim.fn.synIDattr(trans_id, "name")
    if type(syn_group) == "string" and syn_group ~= "" then
      local color = get_hl_color(syn_group, "fg")
      if color then
        return color
      end
    end
  end

  local cursor = { line - 1, col - 1 }

  if vim.bo.buftype == "" and vim.b.ts_highlight then
    local ok, captures =
      pcall(vim.treesitter.get_captures_at_pos, 0, cursor[1], cursor[2])
    if ok and type(captures) == "table" then
      local ts_hl_group
      for _, capture in pairs(captures) do
        ts_hl_group = "@" .. capture.capture .. "." .. capture.lang
      end
      if ts_hl_group then
        local color = get_hl_color(ts_hl_group, "fg")
        if color then
          return color
        end
      end
    end
  end

  if vim.bo.buftype ~= "" and vim.bo.buftype ~= "acwrite" then
    return nil
  end

  local extmarks = vim.api.nvim_buf_get_extmarks(
    0,
    -1,
    cursor,
    cursor,
    { details = true, overlap = true, limit = 32 }
  )
  for _, extmark in ipairs(extmarks) do
    local details = extmark[4]
    local hl_group = details and details.hl_group
    if hl_group then
      local color = get_hl_color(hl_group, "fg")
      if color then
        return color
      end
    end
  end

  return nil
end

function M.background_allowed_mask(request)
  local start_row = request[1]
  local row_count = request[2]
  local max_col = request[3]
  local braille_min = request[4]
  local braille_max = request[5]
  local octant_min = request[6]
  local octant_max = request[7]
  local result = {}
  local index = 1
  for row = start_row, start_row + row_count - 1 do
    for col = 1, max_col do
      local code = vim.fn.screenchar(row, col)
      result[index] = code == 32
        or (code >= braille_min and code <= braille_max)
        or (code >= octant_min and code <= octant_max)
      index = index + 1
    end
  end
  return result
end

return M

local M = {}
local NO_HL_COLOR = {}
local hl_color_cache_generation
local hl_fg_cache = {}

local function reset_hl_color_cache(colorscheme_generation)
  if hl_color_cache_generation == colorscheme_generation then
    return
  end
  hl_color_cache_generation = colorscheme_generation
  hl_fg_cache = {}
end

local function get_hl_fg(group)
  local cached = hl_fg_cache[group]
  if cached ~= nil then
    return cached == NO_HL_COLOR and nil or cached
  end

  local ok, hl = pcall(vim.api.nvim_get_hl, 0, { name = group, link = false })
  local color = ok and type(hl.fg) == "number" and hl.fg or nil
  hl_fg_cache[group] = color or NO_HL_COLOR
  return color
end

local function parse_hex_color(value)
  if type(value) ~= "string" then
    return nil
  end
  local hex = value:match("^#?(%x%x%x%x%x%x)$")
  if not hex then
    return nil
  end
  return tonumber(hex, 16)
end

function M.cursor_color_at_cursor(colorscheme_generation)
  reset_hl_color_cache(colorscheme_generation)

  local line = vim.fn.line(".")
  local col = vim.fn.col(".")

  local syn_id = vim.fn.synID(line, col, 1)
  if type(syn_id) == "number" and syn_id > 0 then
    local trans_id = vim.fn.synIDtrans(syn_id)
    local syn_color = parse_hex_color(vim.fn.synIDattr(trans_id, "fg#"))
    if syn_color then
      return syn_color
    end

    local syn_group = vim.fn.synIDattr(trans_id, "name")
    if type(syn_group) == "string" and syn_group ~= "" then
      local color = get_hl_fg(syn_group)
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
        local color = get_hl_fg(ts_hl_group)
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
      local color = get_hl_fg(hl_group)
      if color then
        return color
      end
    end
  end

  return nil
end

function M.background_allowed_mask(request)
  local braille_min = request[1]
  local braille_max = request[2]
  local octant_min = request[3]
  local octant_max = request[4]
  local packed = {}
  local packed_index = 1
  local packed_byte = 0
  local packed_bit = 0
  for index = 5, #request, 2 do
    local row = request[index]
    local col = request[index + 1]
    local code = vim.fn.screenchar(row, col)
    local allowed = code == 32
      or (code >= braille_min and code <= braille_max)
      or (code >= octant_min and code <= octant_max)
    if allowed then
      packed_byte = packed_byte + (2 ^ packed_bit)
    end
    packed_bit = packed_bit + 1
    if packed_bit == 8 then
      packed[packed_index] = packed_byte
      packed_index = packed_index + 1
      packed_byte = 0
      packed_bit = 0
    end
  end
  if packed_bit ~= 0 then
    packed[packed_index] = packed_byte
  end
  return packed
end

return M

local M = {}

local function parse_winhighlight(value)
  if type(value) ~= "string" or value == "" then
    return nil
  end

  local mappings = {}
  for entry in string.gmatch(value, "[^,]+") do
    local from, to = entry:match("^([^:]+):([^:]+)$")
    if from ~= nil and from ~= "" and to ~= nil and to ~= "" then
      mappings[from] = to
    end
  end

  if next(mappings) == nil then
    return nil
  end

  return mappings
end

local function current_highlight_context()
  local winid = vim.api.nvim_get_current_win()
  local ok, window_hl_ns = pcall(vim.api.nvim_get_hl_ns, { winid = winid })
  local has_window_namespace =
    ok and type(window_hl_ns) == "number" and window_hl_ns >= 0

  return {
    has_window_namespace = has_window_namespace,
    hl_ns = has_window_namespace and window_hl_ns or 0,
    winhighlight = parse_winhighlight(vim.wo[winid].winhighlight),
  }
end

local function highlight_group_name(group)
  local group_type = type(group)
  if group_type == "string" then
    if group == "" then
      return nil
    end
    return group
  end

  if group_type ~= "number" then
    return nil
  end

  local name = vim.fn.synIDattr(group, "name")
  if type(name) ~= "string" or name == "" then
    return nil
  end

  return name
end

local function remapped_highlight_group(group, hl_context)
  local mappings = hl_context and hl_context.winhighlight
  if mappings == nil then
    return group
  end

  local group_name = highlight_group_name(group)
  local mapped_group = group_name and mappings[group_name]
  if type(mapped_group) ~= "string" or mapped_group == "" then
    return group
  end

  return mapped_group
end

local function hl_lookup_opts(group)
  local group_type = type(group)
  if group_type ~= "string" and group_type ~= "number" then
    return nil
  end

  local opts = { link = false }
  if group_type == "number" then
    opts.id = group
  else
    opts.name = group
  end
  opts.create = false
  return opts
end

local function highlight_group_key(group)
  local group_name = highlight_group_name(group)
  if group_name ~= nil then
    return group_name
  end

  if type(group) == "number" then
    return "#" .. tostring(group)
  end

  return nil
end

local function hl_entry_from_namespace(ns_id, group)
  local opts = hl_lookup_opts(group)
  if opts == nil then
    return nil
  end

  local ok, hl = pcall(vim.api.nvim_get_hl, ns_id, opts)
  return ok and type(hl) == "table" and hl or nil
end

local function hl_fg_from_namespace(ns_id, group, seen_links)
  local hl = hl_entry_from_namespace(ns_id, group)
  if hl == nil then
    return nil
  end

  local link = type(hl.link) == "string" and hl.link or nil
  if link ~= nil and link ~= "" then
    local key = highlight_group_key(group) or link
    seen_links = seen_links or {}
    if seen_links[key] then
      return nil
    end

    seen_links[key] = true
    local linked_color = hl_fg_from_namespace(ns_id, link, seen_links)
    if linked_color ~= nil then
      return linked_color
    end

    if ns_id ~= 0 then
      return hl_fg_from_namespace(0, link, seen_links)
    end

    return nil
  end

  return type(hl.fg) == "number" and hl.fg or nil
end

local function get_hl_fg(group, hl_context)
  local has_window_namespace = hl_context and hl_context.has_window_namespace or false
  local hl_ns = hl_context and hl_context.hl_ns or 0
  if not has_window_namespace then
    local remapped_group = remapped_highlight_group(group, hl_context)
    if remapped_group ~= group then
      local remapped_color = hl_fg_from_namespace(0, remapped_group)
      if remapped_color ~= nil then
        return remapped_color
      end
    end
  end

  local color = hl_fg_from_namespace(hl_ns, group)
  if color ~= nil or hl_ns == 0 then
    return color
  end

  return hl_fg_from_namespace(0, group)
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

local function cursor_color_result(color, used_extmark_fallback)
  return {
    color = color,
    used_extmark_fallback = used_extmark_fallback,
  }
end

local function treesitter_capture_priority(capture)
  local metadata = capture.metadata
  local metadata_by_id = metadata and capture.id and metadata[capture.id]
  local priority = metadata and metadata.priority or metadata_by_id and metadata_by_id.priority
  if priority == nil and vim.hl and vim.hl.priorities then
    priority = vim.hl.priorities.treesitter
  end
  return type(priority) == "number" and priority or 0
end

local function treesitter_capture_groups(capture)
  local base_group = "@" .. capture.capture
  if type(capture.lang) == "string" and capture.lang ~= "" then
    return { base_group .. "." .. capture.lang, base_group }
  end
  return { base_group }
end

local function treesitter_capture_color(capture, hl_context)
  for _, group in ipairs(treesitter_capture_groups(capture)) do
    local color = get_hl_fg(group, hl_context)
    if color then
      return color
    end
  end
  return nil
end

local function treesitter_color_at_cursor(cursor, hl_context)
  if vim.bo.buftype ~= "" or not vim.b.ts_highlight then
    return nil
  end

  local ok, captures =
    pcall(vim.treesitter.get_captures_at_pos, 0, cursor[1], cursor[2])
  if not ok or type(captures) ~= "table" then
    return nil
  end

  local best_color
  local best_priority
  for _, capture in ipairs(captures) do
    local color = treesitter_capture_color(capture, hl_context)
    local priority = treesitter_capture_priority(capture)
    if color and (best_priority == nil or priority > best_priority) then
      best_color = color
      best_priority = priority
    end
  end

  if best_color == nil then
    return nil
  end

  return {
    color = best_color,
    priority = best_priority or 0,
    source_rank = 1,
    used_extmark_fallback = false,
  }
end

local function extmark_priority(details)
  local priority = details and details.priority
  return type(priority) == "number" and priority or 0
end

local function extmark_highlight_groups(details)
  local hl_group = details and details.hl_group
  local hl_group_type = type(hl_group)
  if hl_group_type == "string" or hl_group_type == "number" then
    return { hl_group }
  end

  if hl_group_type ~= "table" or not vim.islist(hl_group) then
    return {}
  end

  local groups = {}
  for index = #hl_group, 1, -1 do
    local group = hl_group[index]
    local group_type = type(group)
    if group_type == "string" or group_type == "number" then
      groups[#groups + 1] = group
    end
  end
  return groups
end

local EXTMARK_OVERLAP_PROBE_SOFT_LIMIT = 32
local EXTMARK_OVERLAP_PROBE_SATURATION_LIMIT = EXTMARK_OVERLAP_PROBE_SOFT_LIMIT + 1

local function overlapping_extmarks_at_cursor(cursor)
  local extmarks = vim.api.nvim_buf_get_extmarks(
    0,
    -1,
    cursor,
    cursor,
    { details = true, overlap = true, limit = EXTMARK_OVERLAP_PROBE_SATURATION_LIMIT }
  )
  if #extmarks < EXTMARK_OVERLAP_PROBE_SATURATION_LIMIT then
    return extmarks
  end

  -- Traversal order is not priority order, so a saturated soft cap could omit the winning overlay.
  return vim.api.nvim_buf_get_extmarks(
    0,
    -1,
    cursor,
    cursor,
    { details = true, overlap = true }
  )
end

local function extmark_color_at_cursor(cursor, hl_context)
  local extmarks = overlapping_extmarks_at_cursor(cursor)

  local extmark_candidates = {}
  for extmark_index, extmark in ipairs(extmarks) do
    local details = extmark[4]
    extmark_candidates[#extmark_candidates + 1] = {
      extmark_index = extmark_index,
      groups = extmark_highlight_groups(details),
      priority = extmark_priority(details),
    }
  end

  table.sort(extmark_candidates, function(left, right)
    if left.priority ~= right.priority then
      return left.priority > right.priority
    end
    return left.extmark_index > right.extmark_index
  end)

  for _, candidate in ipairs(extmark_candidates) do
    for _, group in ipairs(candidate.groups) do
      local color = get_hl_fg(group, hl_context)
      if color then
        return {
          color = color,
          priority = candidate.priority,
          source_rank = 2,
          used_extmark_fallback = true,
        }
      end
    end
  end

  return nil
end

local function syntax_color_at_cursor(line, col, hl_context)
  local syn_id = vim.fn.synID(line, col, 1)
  if type(syn_id) ~= "number" or syn_id <= 0 then
    return nil
  end

  local trans_id = vim.fn.synIDtrans(syn_id)
  local syn_group = vim.fn.synIDattr(trans_id, "name")
  if type(syn_group) == "string" and syn_group ~= "" then
    local syn_group_color = get_hl_fg(syn_group, hl_context)
    if syn_group_color then
      return syn_group_color
    end
  end

  return parse_hex_color(vim.fn.synIDattr(trans_id, "fg#"))
end

local function better_overlay_candidate(left, right)
  if left == nil then
    return right
  end

  if right == nil then
    return left
  end

  if left.priority ~= right.priority then
    if left.priority > right.priority then
      return left
    end
    return right
  end

  if left.source_rank ~= right.source_rank then
    if left.source_rank > right.source_rank then
      return left
    end
    return right
  end

  return left
end

function M.cursor_color_at_cursor(colorscheme_generation, allow_extmark_fallback)
  local hl_context = current_highlight_context()
  local line = vim.fn.line(".")
  local col = vim.fn.col(".")
  local syntax_color = syntax_color_at_cursor(line, col, hl_context)

  local cursor = { line - 1, col - 1 }
  local overlay_candidate = treesitter_color_at_cursor(cursor, hl_context)

  if vim.bo.buftype ~= "" and vim.bo.buftype ~= "acwrite" then
    if overlay_candidate then
      return cursor_color_result(
        overlay_candidate.color,
        overlay_candidate.used_extmark_fallback
      )
    end
    return cursor_color_result(syntax_color, false)
  end

  if allow_extmark_fallback then
    overlay_candidate = better_overlay_candidate(
      overlay_candidate,
      extmark_color_at_cursor(cursor, hl_context)
    )
  end

  if overlay_candidate then
    return cursor_color_result(
      overlay_candidate.color,
      overlay_candidate.used_extmark_fallback
    )
  end

  if syntax_color then
    return cursor_color_result(syntax_color, false)
  end

  if not allow_extmark_fallback then
    return cursor_color_result(nil, false)
  end

  return cursor_color_result(nil, true)
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

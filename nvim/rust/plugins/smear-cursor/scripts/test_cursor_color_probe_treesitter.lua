local script_path = debug.getinfo(1, "S").source:sub(2)
local script_dir = vim.fn.fnamemodify(script_path, ":h")
local helpers = dofile(script_dir .. "/lib/probe_test_helpers.lua")
local prepend_runtimepath = helpers.prepend_runtimepath
local assert_probe_result = helpers.assert_probe_result
local reset_probe_module = helpers.reset_probe_module
local with_mocked_syntax = helpers.with_mocked_syntax

local function prepare_cursor_buffer()
  vim.cmd("enew!")
  vim.bo.buftype = ""
  vim.bo.filetype = ""
  vim.b.ts_highlight = true
  vim.cmd("syntax off")
  vim.api.nvim_buf_set_lines(0, 0, -1, false, { "x" })
  vim.api.nvim_win_set_cursor(0, { 1, 0 })
end

local function with_mocked_treesitter(captures, callback)
  local original = vim.treesitter.get_captures_at_pos
  vim.treesitter.get_captures_at_pos = function()
    return captures
  end

  local ok, result = pcall(callback)
  vim.treesitter.get_captures_at_pos = original
  if not ok then
    error(result)
  end
  return result
end

local function test_prefers_highest_priority_capture()
  prepare_cursor_buffer()
  local probes = reset_probe_module()
  vim.api.nvim_set_hl(0, "@smear_priority_high.lua", { fg = "#112233" })
  vim.api.nvim_set_hl(0, "@smear_priority_low.lua", { fg = "#445566" })

  local result = with_mocked_treesitter({
    {
      capture = "smear_priority_high",
      lang = "lua",
      metadata = { priority = 200 },
    },
    {
      capture = "smear_priority_low",
      lang = "lua",
      metadata = { priority = 50 },
    },
  }, function()
    return probes.cursor_color_at_cursor(0, false)
  end)

  assert_probe_result(result, {
    color = 0x112233,
    used_extmark_fallback = false,
  }, "highest priority treesitter capture should win")
end

local function test_falls_back_to_base_capture_group()
  prepare_cursor_buffer()
  local probes = reset_probe_module()
  vim.api.nvim_set_hl(0, "@smear_base_only", { fg = "#abcdef" })

  local result = with_mocked_treesitter({
    {
      capture = "smear_base_only",
      lang = "lua",
      metadata = { priority = 100 },
    },
  }, function()
    return probes.cursor_color_at_cursor(1, false)
  end)

  assert_probe_result(result, {
    color = 0xABCDEF,
    used_extmark_fallback = false,
  }, "base capture group should be used when lang-specific group is missing")
end

local function test_prefers_treesitter_overlay_over_syntax()
  prepare_cursor_buffer()
  local probes = reset_probe_module()
  vim.api.nvim_set_hl(0, "@smear_overlay.lua", { fg = "#102030" })

  local result = with_mocked_syntax({
    id = 1,
    trans_id = 1,
    fg = "#c0ffee",
    name = "SmearSyntaxBase",
  }, function()
    return with_mocked_treesitter({
      {
        capture = "smear_overlay",
        lang = "lua",
        metadata = { priority = 100 },
      },
    }, function()
      return probes.cursor_color_at_cursor(0, false)
    end)
  end)

  assert_probe_result(result, {
    color = 0x102030,
    used_extmark_fallback = false,
  }, "treesitter overlay should beat the syntax base color")
end

local function main()
  prepend_runtimepath(os.getenv("SMEAR_CURSOR_RTP") or "")
  vim.opt.swapfile = false
  test_prefers_highest_priority_capture()
  test_falls_back_to_base_capture_group()
  test_prefers_treesitter_overlay_over_syntax()
  print("SMEAR_CURSOR_PROBE_TREESITTER_OK")
end

local ok, err = pcall(main)
if not ok then
  error(err)
end

vim.cmd("qa!")

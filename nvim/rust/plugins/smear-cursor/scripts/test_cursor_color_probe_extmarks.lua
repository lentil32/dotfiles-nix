local script_path = debug.getinfo(1, "S").source:sub(2)
local script_dir = vim.fn.fnamemodify(script_path, ":h")
local helpers = dofile(script_dir .. "/lib/probe_test_helpers.lua")
local prepend_runtimepath = helpers.prepend_runtimepath
local assert_probe_result = helpers.assert_probe_result
local reset_probe_module = helpers.reset_probe_module
local with_mocked_syntax = helpers.with_mocked_syntax

local function prepare_cursor_buffer()
  vim.cmd("silent! only")
  vim.cmd("enew!")
  vim.bo.buftype = ""
  vim.bo.filetype = ""
  vim.b.ts_highlight = false
  vim.wo.winhighlight = ""
  vim.api.nvim_win_set_hl_ns(0, -1)
  vim.cmd("syntax off")
  vim.api.nvim_buf_set_lines(0, 0, -1, false, { "x" })
  vim.api.nvim_win_set_cursor(0, { 1, 0 })
end

local function with_mocked_extmarks(extmarks, callback)
  local original = vim.api.nvim_buf_get_extmarks
  vim.api.nvim_buf_get_extmarks = function()
    return extmarks
  end

  local ok, result = pcall(callback)
  vim.api.nvim_buf_get_extmarks = original
  if not ok then
    error(result)
  end
  return result
end

local function with_observed_extmark_limits(extmarks, callback)
  local calls = {}
  local result = with_mocked_extmarks(extmarks, function()
    local original = vim.api.nvim_buf_get_extmarks
    vim.api.nvim_buf_get_extmarks = function(buffer, namespace, start_pos, end_pos, opts)
      calls[#calls + 1] = opts and opts.limit or false
      return original(buffer, namespace, start_pos, end_pos, opts)
    end

    local ok, probe_result = pcall(callback)
    vim.api.nvim_buf_get_extmarks = original
    if not ok then
      error(probe_result)
    end
    return probe_result
  end)

  return result, calls
end

local function assert_single_extmark_scan(calls, expected_limit, context)
  if #calls ~= 1 then
    error(context .. " should issue exactly one host scan, got " .. #calls)
  end
  if calls[1] ~= expected_limit then
    error(context .. " should request limit=" .. expected_limit .. ", got " .. tostring(calls[1]))
  end
end

local function probe_in_window(window, callback)
  local ok, result = pcall(vim.api.nvim_win_call, window, callback)
  if not ok then
    error(result)
  end
  return result
end

local function prepare_shared_cursor_windows()
  prepare_cursor_buffer()
  local buffer = vim.api.nvim_get_current_buf()
  local left = vim.api.nvim_get_current_win()
  vim.cmd("vsplit")
  local right = vim.api.nvim_get_current_win()
  vim.api.nvim_win_set_cursor(left, { 1, 0 })
  vim.api.nvim_win_set_cursor(right, { 1, 0 })
  return {
    buffer = buffer,
    left = left,
    right = right,
  }
end

local function test_prefers_highest_priority_extmark()
  prepare_cursor_buffer()
  local probes = reset_probe_module()
  vim.api.nvim_set_hl(0, "SmearExtmarkLow", { fg = "#112233" })
  vim.api.nvim_set_hl(0, "SmearExtmarkHigh", { fg = "#445566" })

  local result = with_mocked_extmarks({
    { 1, 0, 0, { end_col = 1, hl_group = "SmearExtmarkLow", priority = 20 } },
    { 2, 0, 0, { end_col = 1, hl_group = "SmearExtmarkHigh", priority = 90 } },
  }, function()
    return probes.cursor_color_at_cursor(true)
  end)

  assert_probe_result(result, {
    color = 0x445566,
    used_extmark_fallback = true,
  }, "highest priority extmark highlight should win")
end

local function test_uses_topmost_visible_group_from_stacked_extmark()
  prepare_cursor_buffer()
  local probes = reset_probe_module()
  vim.api.nvim_set_hl(0, "SmearExtmarkStackLow", { fg = "#111111" })
  vim.api.nvim_set_hl(0, "SmearExtmarkStackVisible", { fg = "#223344" })

  local result = with_mocked_extmarks({
    {
      3,
      0,
      0,
      {
        end_col = 1,
        hl_group = {
          "SmearExtmarkStackLow",
          "SmearExtmarkStackVisible",
          "SmearExtmarkStackTopMissing",
        },
        priority = 120,
      },
    },
  }, function()
    return probes.cursor_color_at_cursor(true)
  end)

  assert_probe_result(result, {
    color = 0x223344,
    used_extmark_fallback = true,
  }, "stacked extmark highlight should resolve from the topmost visible group")
end

local function test_prefers_extmark_overlay_over_syntax()
  prepare_cursor_buffer()
  local probes = reset_probe_module()
  vim.api.nvim_set_hl(0, "SmearExtmarkOverlay", { fg = "#304050" })

  local result = with_mocked_syntax({
    id = 1,
    trans_id = 1,
    fg = "#c0ffee",
    name = "SmearSyntaxBase",
  }, function()
    return with_mocked_extmarks({
      { 4, 0, 0, { end_col = 1, hl_group = "SmearExtmarkOverlay", priority = 20 } },
    }, function()
      return probes.cursor_color_at_cursor(true)
    end)
  end)

  assert_probe_result(result, {
    color = 0x304050,
    used_extmark_fallback = true,
  }, "extmark overlay should beat the syntax base color")
end

local function test_reads_updated_highlight_without_colorscheme_reload()
  prepare_cursor_buffer()
  local probes = reset_probe_module()
  vim.api.nvim_set_hl(0, "SmearExtmarkMutable", { fg = "#112233" })

  local results = with_mocked_extmarks({
    { 5, 0, 0, { end_col = 1, hl_group = "SmearExtmarkMutable", priority = 30 } },
  }, function()
    local initial = probes.cursor_color_at_cursor(true)
    vim.api.nvim_set_hl(0, "SmearExtmarkMutable", { fg = "#445566" })
    local updated = probes.cursor_color_at_cursor(true)
    return {
      initial = initial,
      updated = updated,
    }
  end)

  assert_probe_result(results.initial, {
    color = 0x112233,
    used_extmark_fallback = true,
  }, "initial probe should see the original highlight fg")
  assert_probe_result(results.updated, {
    color = 0x445566,
    used_extmark_fallback = true,
  }, "probe should see highlight fg updates without a colorscheme generation change")
end

local function test_reads_group_defined_after_initial_probe()
  prepare_cursor_buffer()
  local probes = reset_probe_module()

  local results = with_mocked_extmarks({
    { 6, 0, 0, { end_col = 1, hl_group = "SmearExtmarkLateDefined", priority = 30 } },
  }, function()
    local initial = probes.cursor_color_at_cursor(true)
    vim.api.nvim_set_hl(0, "SmearExtmarkLateDefined", { fg = "#778899" })
    local updated = probes.cursor_color_at_cursor(true)
    return {
      initial = initial,
      updated = updated,
    }
  end)

  assert_probe_result(results.initial, {
    color = nil,
    used_extmark_fallback = true,
  }, "initial probe should miss when the extmark group is undefined")
  assert_probe_result(results.updated, {
    color = 0x778899,
    used_extmark_fallback = true,
  }, "probe should see groups that become defined without a colorscheme generation change")
end

local function test_resolves_winhighlight_per_window()
  local probes = reset_probe_module()
  local windows = prepare_shared_cursor_windows()
  local namespace = vim.api.nvim_create_namespace("smear_cursor_window_extmarks")
  vim.api.nvim_set_hl(0, "SmearWindowBase", { fg = "#101010" })
  vim.api.nvim_set_hl(0, "SmearWindowLeft", { fg = "#112233" })
  vim.api.nvim_set_hl(0, "SmearWindowRight", { fg = "#445566" })
  vim.wo[windows.left].winhighlight = "SmearWindowBase:SmearWindowLeft"
  vim.wo[windows.right].winhighlight = "SmearWindowBase:SmearWindowRight"
  vim.api.nvim_buf_set_extmark(windows.buffer, namespace, 0, 0, {
    end_col = 1,
    hl_group = "SmearWindowBase",
  })

  local left_result = probe_in_window(windows.left, function()
    return probes.cursor_color_at_cursor(true)
  end)
  local right_result = probe_in_window(windows.right, function()
    return probes.cursor_color_at_cursor(true)
  end)

  assert_probe_result(left_result, {
    color = 0x112233,
    used_extmark_fallback = true,
  }, "winhighlight remap should resolve in the left window")
  assert_probe_result(right_result, {
    color = 0x445566,
    used_extmark_fallback = true,
  }, "winhighlight remap should resolve in the right window")
end

local function test_resolves_window_highlight_namespaces_per_window()
  local probes = reset_probe_module()
  local windows = prepare_shared_cursor_windows()
  local extmark_namespace = vim.api.nvim_create_namespace("smear_cursor_window_ns_extmarks")
  local left_hl_ns = vim.api.nvim_create_namespace("smear_cursor_left_hl_ns")
  local right_hl_ns = vim.api.nvim_create_namespace("smear_cursor_right_hl_ns")
  vim.api.nvim_set_hl(left_hl_ns, "SmearWindowNsGroup", { fg = "#123456" })
  vim.api.nvim_set_hl(right_hl_ns, "SmearWindowNsGroup", { fg = "#654321" })
  vim.api.nvim_win_set_hl_ns(windows.left, left_hl_ns)
  vim.api.nvim_win_set_hl_ns(windows.right, right_hl_ns)
  vim.api.nvim_buf_set_extmark(windows.buffer, extmark_namespace, 0, 0, {
    end_col = 1,
    hl_group = "SmearWindowNsGroup",
  })

  local left_result = probe_in_window(windows.left, function()
    return probes.cursor_color_at_cursor(true)
  end)
  local right_result = probe_in_window(windows.right, function()
    return probes.cursor_color_at_cursor(true)
  end)

  assert_probe_result(left_result, {
    color = 0x123456,
    used_extmark_fallback = true,
  }, "window highlight namespace should resolve in the left window")
  assert_probe_result(right_result, {
    color = 0x654321,
    used_extmark_fallback = true,
  }, "window highlight namespace should resolve in the right window")
end

local function test_extmark_probe_stays_bounded_when_overlap_limit_saturates()
  prepare_cursor_buffer()
  local probes = reset_probe_module()
  vim.api.nvim_set_hl(0, "SmearExtmarkInsideLimit", { fg = "#111111" })
  vim.api.nvim_set_hl(0, "SmearExtmarkPastLimit", { fg = "#abcdef" })

  local extmarks = {}
  for index = 1, 32 do
    extmarks[index] = {
      index,
      0,
      0,
      { end_col = 1, hl_group = "SmearExtmarkInsideLimit", priority = 1 },
    }
  end
  extmarks[33] = {
    33,
    0,
    0,
    { end_col = 1, hl_group = "SmearExtmarkPastLimit", priority = 10000 },
  }

  local result, calls = with_observed_extmark_limits(extmarks, function()
    return probes.cursor_color_at_cursor(true)
  end)
  assert_single_extmark_scan(calls, 33, "bounded extmark probe")

  assert_probe_result(result, {
    color = nil,
    used_extmark_fallback = true,
  }, "saturated extmark probe should degrade instead of trusting a partial priority set")
end

local function test_extmark_probe_trusts_exact_overlap_limit()
  prepare_cursor_buffer()
  local probes = reset_probe_module()
  vim.api.nvim_set_hl(0, "SmearExtmarkInsideLimitLow", { fg = "#111111" })
  vim.api.nvim_set_hl(0, "SmearExtmarkInsideLimitHigh", { fg = "#abcdef" })

  local extmarks = {}
  for index = 1, 31 do
    extmarks[index] = {
      index,
      0,
      0,
      { end_col = 1, hl_group = "SmearExtmarkInsideLimitLow", priority = 1 },
    }
  end
  extmarks[32] = {
    32,
    0,
    0,
    { end_col = 1, hl_group = "SmearExtmarkInsideLimitHigh", priority = 10000 },
  }

  local result, calls = with_observed_extmark_limits(extmarks, function()
    return probes.cursor_color_at_cursor(true)
  end)
  assert_single_extmark_scan(calls, 33, "exact-limit extmark probe")

  assert_probe_result(result, {
    color = 0xabcdef,
    used_extmark_fallback = true,
  }, "exact-limit extmark probe should trust the complete candidate set")
end

local function main()
  prepend_runtimepath(os.getenv("SMEAR_CURSOR_RTP") or "")
  vim.opt.swapfile = false
  test_prefers_highest_priority_extmark()
  test_uses_topmost_visible_group_from_stacked_extmark()
  test_prefers_extmark_overlay_over_syntax()
  test_reads_updated_highlight_without_colorscheme_reload()
  test_reads_group_defined_after_initial_probe()
  test_resolves_winhighlight_per_window()
  test_resolves_window_highlight_namespaces_per_window()
  test_extmark_probe_stays_bounded_when_overlap_limit_saturates()
  test_extmark_probe_trusts_exact_overlap_limit()
  print("SMEAR_CURSOR_PROBE_EXTMARKS_OK")
end

local ok, err = pcall(main)
if not ok then
  error(err)
end

vim.cmd("qa!")

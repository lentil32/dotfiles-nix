local uv = vim.uv or vim.loop

-- Headless Neovim perf harness.
-- Usage: run via scripts/run_perf_window_switch.sh and override parameters with:
-- `SMEAR_WINDOWS`, `SMEAR_STRESS_ITERATIONS`, `SMEAR_STRESS_ROUNDS`,
-- `SMEAR_BETWEEN_BUFFERS`, `SMEAR_MAX_RECOVERY_RATIO`, `SMEAR_MAX_STRESS_RATIO`,
-- `SMEAR_SETTLE_WAIT_MS`,
-- `SMEAR_DRAIN_EVERY`, `SMEAR_KEYS_PER_SWITCH`, `SMEAR_DELAY_EVENT_TO_SMEAR`,
-- and `SMEAR_DELAY_AFTER_KEY`.

local function getenv_string(name, default_value)
  local value = vim.env[name]
  if value == nil or value == "" then
    return default_value
  end
  return value
end

local function getenv_positive_integer(name, default_value)
  local raw_value = getenv_string(name, nil)
  if raw_value == nil then
    return default_value
  end

  local parsed = tonumber(raw_value)
  if parsed == nil then
    error(string.format("%s must be an integer, got %q", name, raw_value))
  end

  local rounded = math.floor(parsed)
  if rounded < 1 or parsed ~= rounded then
    error(string.format("%s must be a positive integer, got %q", name, raw_value))
  end

  return rounded
end

local function getenv_non_negative_number(name, default_value)
  local raw_value = getenv_string(name, nil)
  if raw_value == nil then
    return default_value
  end

  local parsed = tonumber(raw_value)
  if parsed == nil or parsed < 0 then
    error(string.format("%s must be a non-negative number, got %q", name, raw_value))
  end

  return parsed
end

local function getenv_non_negative_integer(name, default_value)
  local raw_value = getenv_string(name, nil)
  if raw_value == nil then
    return default_value
  end

  local parsed = tonumber(raw_value)
  if parsed == nil then
    error(string.format("%s must be an integer, got %q", name, raw_value))
  end

  local rounded = math.floor(parsed)
  if rounded < 0 or parsed ~= rounded then
    error(string.format("%s must be a non-negative integer, got %q", name, raw_value))
  end

  return rounded
end

local function getenv_bool(name, default_value)
  local raw_value = getenv_string(name, nil)
  if raw_value == nil then
    return default_value
  end

  if raw_value == "1" or raw_value == "true" or raw_value == "TRUE" then
    return true
  end
  if raw_value == "0" or raw_value == "false" or raw_value == "FALSE" then
    return false
  end

  error(string.format("%s must be one of 1, 0, true, false, got %q", name, raw_value))
end

local function prepend_runtimepath(path)
  if path == "" then
    return
  end

  if not vim.startswith(vim.o.runtimepath, path .. ",") and vim.o.runtimepath ~= path then
    vim.o.runtimepath = path .. "," .. vim.o.runtimepath
  end
end

local function append_package_cpath(path)
  if path == "" then
    return
  end
  package.cpath = package.cpath .. ";" .. path
end

local function count_floating_windows()
  local floating_windows = 0
  for _, win in ipairs(vim.api.nvim_list_wins()) do
    local win_config = vim.api.nvim_win_get_config(win)
    if win_config.relative ~= "" then
      floating_windows = floating_windows + 1
    end
  end
  return floating_windows
end

local function count_smear_floating_windows()
  local floating_windows = 0
  for _, win in ipairs(vim.api.nvim_list_wins()) do
    local win_config = vim.api.nvim_win_get_config(win)
    if win_config.relative == "" then
      goto continue
    end

    local buffer = vim.api.nvim_win_get_buf(win)
    if not vim.api.nvim_buf_is_valid(buffer) then
      goto continue
    end

    local filetype = vim.bo[buffer].filetype
    local buftype = vim.bo[buffer].buftype
    if filetype == "smear-cursor" and buftype == "nofile" then
      floating_windows = floating_windows + 1
    end

    ::continue::
  end
  return floating_windows
end

local function create_workload_buffer(line_count)
  local buffer = vim.api.nvim_create_buf(true, false)
  local lines = {}
  for index = 1, line_count do
    lines[index] = string.format("line-%05d", index)
  end
  vim.api.nvim_buf_set_lines(buffer, 0, -1, false, lines)
  return buffer
end

local function create_split_windows(base_buffer, requested_windows)
  vim.api.nvim_set_current_buf(base_buffer)
  for _ = 2, requested_windows do
    vim.cmd("vsplit")
    vim.api.nvim_set_current_buf(base_buffer)
  end
  vim.cmd("wincmd =")

  local windows = {}
  for _, win in ipairs(vim.api.nvim_tabpage_list_wins(0)) do
    local win_config = vim.api.nvim_win_get_config(win)
    if win_config.relative == "" then
      windows[#windows + 1] = win
    end
  end

  for index, win in ipairs(windows) do
    local line = math.min(index * 11, 1999)
    vim.api.nvim_win_set_cursor(win, { line, 0 })
  end

  return windows
end

local function drain_event_loop_once()
  vim.wait(1, function()
    return false
  end, 1)
end

local function run_switch_phase(label, windows, iterations, drain_every, smear, keys_per_switch)
  local start_ns = uv.hrtime()
  for iteration = 1, iterations do
    local window_index = ((iteration - 1) % #windows) + 1
    if keys_per_switch > 0 then
      for _ = 1, keys_per_switch do
        smear.on_key()
      end
    end
    vim.api.nvim_set_current_win(windows[window_index])
    if drain_every > 0 and (iteration % drain_every) == 0 then
      drain_event_loop_once()
    end
  end

  if drain_every > 0 then
    drain_event_loop_once()
  end

  local elapsed_ns = uv.hrtime() - start_ns
  local elapsed_ms = elapsed_ns / 1e6
  local avg_us = (elapsed_ns / iterations) / 1e3
  local floating_windows = count_floating_windows()
  local smear_floating_windows = count_smear_floating_windows()
  local lua_memory_kib = collectgarbage("count")

  print(
    string.format(
      "PERF_PHASE name=%s iterations=%d elapsed_ms=%.3f avg_us=%.3f floating_windows=%d smear_floating_windows=%d lua_memory_kib=%.1f",
      label,
      iterations,
      elapsed_ms,
      avg_us,
      floating_windows,
      smear_floating_windows,
      lua_memory_kib
    )
  )

  return {
    elapsed_ns = elapsed_ns,
    avg_us = avg_us,
    floating_windows = floating_windows,
    smear_floating_windows = smear_floating_windows,
    lua_memory_kib = lua_memory_kib,
  }
end

local function wait_for_cleanup(delay_ms)
  if delay_ms <= 0 then
    return
  end
  vim.wait(delay_ms, function()
    return false
  end, 10)
end

local function main()
  prepend_runtimepath(getenv_string("SMEAR_CURSOR_RTP", ""))
  append_package_cpath(getenv_string("SMEAR_CURSOR_CPATH", ""))

  local ok, smear = pcall(require, "rs_smear_cursor")
  if not ok then
    error("failed to require rs_smear_cursor: " .. tostring(smear))
  end

  local windows_count = getenv_positive_integer("SMEAR_WINDOWS", 8)
  local warmup_iterations = getenv_positive_integer("SMEAR_WARMUP_ITERATIONS", 500)
  local baseline_iterations = getenv_positive_integer("SMEAR_BASELINE_ITERATIONS", 3000)
  local stress_iterations = getenv_positive_integer("SMEAR_STRESS_ITERATIONS", 20000)
  local stress_rounds = getenv_positive_integer("SMEAR_STRESS_ROUNDS", 4)
  local recovery_iterations = getenv_positive_integer("SMEAR_RECOVERY_ITERATIONS", baseline_iterations)
  local settle_wait_ms = getenv_non_negative_number("SMEAR_SETTLE_WAIT_MS", 1200)
  local max_recovery_ratio = getenv_non_negative_number("SMEAR_MAX_RECOVERY_RATIO", 1.4)
  local max_stress_ratio = getenv_non_negative_number("SMEAR_MAX_STRESS_RATIO", 2.0)
  local max_floating_windows = getenv_positive_integer("SMEAR_MAX_FLOATING_WINDOWS", 256)
  local smear_between_buffers = getenv_bool("SMEAR_BETWEEN_BUFFERS", false)
  local drain_every = getenv_non_negative_integer("SMEAR_DRAIN_EVERY", 16)
  local keys_per_switch = getenv_non_negative_integer("SMEAR_KEYS_PER_SWITCH", 0)
  local delay_event_to_smear = getenv_non_negative_number("SMEAR_DELAY_EVENT_TO_SMEAR", 1)
  local delay_after_key = getenv_non_negative_number("SMEAR_DELAY_AFTER_KEY", 5)

  smear.setup({
    logging_level = 0,
    particles_enabled = false,
    smear_between_buffers = smear_between_buffers,
    smear_between_neighbor_lines = true,
    delay_event_to_smear = delay_event_to_smear,
    delay_after_key = delay_after_key,
    time_interval = 16,
  })

  local workload_buffer = create_workload_buffer(2000)
  local windows = create_split_windows(workload_buffer, windows_count)
  if #windows < 2 then
    error("need at least 2 windows for this harness")
  end

  print(
    string.format(
      "PERF_CONFIG windows=%d warmup_iterations=%d baseline_iterations=%d stress_iterations=%d stress_rounds=%d recovery_iterations=%d settle_wait_ms=%.0f smear_between_buffers=%s max_recovery_ratio=%.3f max_stress_ratio=%.3f drain_every=%d keys_per_switch=%d delay_event_to_smear=%.3f delay_after_key=%.3f",
      #windows,
      warmup_iterations,
      baseline_iterations,
      stress_iterations,
      stress_rounds,
      recovery_iterations,
      settle_wait_ms,
      tostring(smear_between_buffers),
      max_recovery_ratio,
      max_stress_ratio,
      drain_every,
      keys_per_switch,
      delay_event_to_smear,
      delay_after_key
    )
  )

  run_switch_phase("warmup", windows, warmup_iterations, drain_every, smear, keys_per_switch)
  local baseline = run_switch_phase("baseline", windows, baseline_iterations, drain_every, smear, keys_per_switch)

  local stress_results = {}
  for round = 1, stress_rounds do
    local result = run_switch_phase(
      string.format("stress_%d", round),
      windows,
      stress_iterations,
      drain_every,
      smear,
      keys_per_switch
    )
    stress_results[#stress_results + 1] = result
  end

  wait_for_cleanup(settle_wait_ms)

  local post_wait_floating_windows = count_floating_windows()
  local post_wait_smear_floating_windows = count_smear_floating_windows()
  local recovery = run_switch_phase("recovery", windows, recovery_iterations, drain_every, smear, keys_per_switch)
  local recovery_ratio = recovery.avg_us / baseline.avg_us

  local stress_max_avg_us = baseline.avg_us
  local stress_tail_avg_us = baseline.avg_us
  for _, result in ipairs(stress_results) do
    if result.avg_us > stress_max_avg_us then
      stress_max_avg_us = result.avg_us
    end
    stress_tail_avg_us = result.avg_us
  end
  local stress_max_ratio = stress_max_avg_us / baseline.avg_us
  local stress_tail_ratio = stress_tail_avg_us / baseline.avg_us

  print(
    string.format(
      "PERF_STRESS_SUMMARY max_avg_us=%.3f tail_avg_us=%.3f max_ratio=%.3f tail_ratio=%.3f",
      stress_max_avg_us,
      stress_tail_avg_us,
      stress_max_ratio,
      stress_tail_ratio
    )
  )

  print(
    string.format(
      "PERF_SUMMARY baseline_avg_us=%.3f recovery_avg_us=%.3f recovery_ratio=%.3f post_wait_floating_windows=%d post_wait_smear_floating_windows=%d",
      baseline.avg_us,
      recovery.avg_us,
      recovery_ratio,
      post_wait_floating_windows,
      post_wait_smear_floating_windows
    )
  )

  if post_wait_floating_windows > max_floating_windows then
    error(
      string.format(
        "floating windows still high after settle wait: expected <= %d, got %d",
        max_floating_windows,
        post_wait_floating_windows
      )
    )
  end

  if post_wait_smear_floating_windows > max_floating_windows then
    error(
      string.format(
        "smear floating windows still high after settle wait: expected <= %d, got %d",
        max_floating_windows,
        post_wait_smear_floating_windows
      )
    )
  end

  if stress_max_ratio > max_stress_ratio then
    error(string.format("stress ratio above threshold: expected <= %.3f, got %.3f", max_stress_ratio, stress_max_ratio))
  end

  if recovery_ratio > max_recovery_ratio then
    error(
      string.format("recovery ratio above threshold: expected <= %.3f, got %.3f", max_recovery_ratio, recovery_ratio)
    )
  end

  vim.cmd("qa!")
end

main()

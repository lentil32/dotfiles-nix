local uv = vim.uv or vim.loop

-- Headless Neovim perf harness.
-- Usage: run via scripts/run_perf_window_switch.sh and override parameters with:
-- `SMEAR_WINDOWS`, `SMEAR_LINE_COUNT`, `SMEAR_STRESS_ITERATIONS`, `SMEAR_STRESS_ROUNDS`,
-- `SMEAR_BETWEEN_BUFFERS`, `SMEAR_MAX_RECOVERY_RATIO`, `SMEAR_MAX_STRESS_RATIO`,
-- `SMEAR_SETTLE_WAIT_MS`, `SMEAR_RECOVERY_MODE`, `SMEAR_COLD_WAIT_TIMEOUT_MS`,
-- `SMEAR_REQUIRE_COLD_RECOVERY`,
-- `SMEAR_RECOVERY_POLL_MS`, `SMEAR_LOGGING_LEVEL`, `SMEAR_SCENARIO_NAME`,
-- `SMEAR_DRAIN_EVERY` and `SMEAR_DELAY_EVENT_TO_SMEAR`.
-- Logging note: this plugin uses lower numbers for more logging and `4` for the least verbose
-- mode, so the perf harness keeps `logging_level = 4` when it wants logging effectively off.

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

local function prepend_package_cpath(path)
  if path == "" then
    return
  end

  -- Surprising: appending the working-tree cdylib path lets an installed plugin shadow the local
  -- build. Prepend here so the harness always measures the code under test.
  if not vim.startswith(package.cpath, path .. ";") and package.cpath ~= path then
    package.cpath = path .. ";" .. package.cpath
  end
end

local function is_counted_floating_window(win_config, visible_only)
  if win_config.relative == "" then
    return false
  end

  return not visible_only or not win_config.hide
end

local function count_floating_windows(visible_only)
  local floating_windows = 0
  for _, win in ipairs(vim.api.nvim_list_wins()) do
    local win_config = vim.api.nvim_win_get_config(win)
    if is_counted_floating_window(win_config, visible_only) then
      floating_windows = floating_windows + 1
    end
  end
  return floating_windows
end

local function count_smear_floating_windows(visible_only)
  local floating_windows = 0
  for _, win in ipairs(vim.api.nvim_list_wins()) do
    local win_config = vim.api.nvim_win_get_config(win)
    if not is_counted_floating_window(win_config, visible_only) then
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
  -- Surprising: a listed non-scratch `[No Name]` buffer can try to create a swapfile in headless
  -- perf runs. Keep the synthetic workload buffer scratch-only so the harness measures recovery
  -- behavior instead of swapfile side effects.
  local buffer = vim.api.nvim_create_buf(false, true)
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

local function emit_line(line)
  io.stdout:write(line .. "\n")
  io.stdout:flush()
end

local function parse_diagnostics_fields(raw_diagnostics)
  local fields = {}
  for token in string.gmatch(raw_diagnostics, "%S+") do
    local key, value = token:match("^([^=]+)=(.*)$")
    if key ~= nil then
      fields[key] = value
    end
  end
  return fields
end

local function read_diagnostics(smear)
  local raw = smear.diagnostics()
  return {
    raw = raw,
    fields = parse_diagnostics_fields(raw),
  }
end

local function run_switch_phase(label, windows, iterations, drain_every)
  local start_ns = uv.hrtime()
  for iteration = 1, iterations do
    local window_index = ((iteration - 1) % #windows) + 1
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
  local floating_windows = count_floating_windows(false)
  local visible_floating_windows = count_floating_windows(true)
  local smear_floating_windows = count_smear_floating_windows(false)
  local visible_smear_floating_windows = count_smear_floating_windows(true)
  local lua_memory_kib = collectgarbage("count")

  emit_line(
    string.format(
      "PERF_PHASE name=%s iterations=%d elapsed_ms=%.3f avg_us=%.3f floating_windows=%d visible_floating_windows=%d smear_floating_windows=%d visible_smear_floating_windows=%d lua_memory_kib=%.1f",
      label,
      iterations,
      elapsed_ms,
      avg_us,
      floating_windows,
      visible_floating_windows,
      smear_floating_windows,
      visible_smear_floating_windows,
      lua_memory_kib
    )
  )

  return {
    elapsed_ns = elapsed_ns,
    avg_us = avg_us,
    floating_windows = floating_windows,
    visible_floating_windows = visible_floating_windows,
    smear_floating_windows = smear_floating_windows,
    visible_smear_floating_windows = visible_smear_floating_windows,
    lua_memory_kib = lua_memory_kib,
  }
end

local function print_diagnostics(label, smear)
  local diagnostics = read_diagnostics(smear)
  emit_line(string.format("PERF_DIAGNOSTICS phase=%s %s", label, diagnostics.raw))
  return diagnostics
end

local function wait_for_cleanup(delay_ms)
  if delay_ms <= 0 then
    return
  end
  vim.wait(delay_ms, function()
    return false
  end, 10)
end

local function validate_recovery_mode(recovery_mode)
  if recovery_mode == "cold" or recovery_mode == "fixed" then
    return recovery_mode
  end

  error(string.format("SMEAR_RECOVERY_MODE must be 'cold' or 'fixed', got %q", recovery_mode))
end

local function wait_for_recovery(
  smear,
  recovery_mode,
  settle_wait_ms,
  cold_wait_timeout_ms,
  recovery_poll_ms,
  require_cold_recovery
)
  local start_ns = uv.hrtime()
  local last_diagnostics = read_diagnostics(smear)
  local reached_cold = last_diagnostics.fields.cleanup_thermal == "cold"
  local timed_out = false

  if recovery_mode == "fixed" then
    wait_for_cleanup(settle_wait_ms)
    last_diagnostics = read_diagnostics(smear)
    reached_cold = last_diagnostics.fields.cleanup_thermal == "cold"
  else
    if not reached_cold then
      reached_cold = vim.wait(cold_wait_timeout_ms, function()
        last_diagnostics = read_diagnostics(smear)
        return last_diagnostics.fields.cleanup_thermal == "cold"
      end, recovery_poll_ms)
      timed_out = not reached_cold
      if timed_out then
        last_diagnostics = read_diagnostics(smear)
        reached_cold = last_diagnostics.fields.cleanup_thermal == "cold"
      end
    end
  end

  local elapsed_ms = (uv.hrtime() - start_ns) / 1e6
  local cleanup_thermal = last_diagnostics.fields.cleanup_thermal or "unknown"
  local compaction_target_reached = last_diagnostics.fields.compaction_target_reached or "unknown"
  local queue_total_backlog = last_diagnostics.fields.queue_total_backlog or "unknown"
  local pool_total_windows = last_diagnostics.fields.pool_total_windows or "unknown"

  emit_line(
    string.format(
      "PERF_RECOVERY_WAIT mode=%s elapsed_ms=%.3f reached_cold=%s timed_out=%s cleanup_thermal=%s compaction_target_reached=%s queue_total_backlog=%s pool_total_windows=%s",
      recovery_mode,
      elapsed_ms,
      tostring(reached_cold),
      tostring(timed_out),
      cleanup_thermal,
      compaction_target_reached,
      queue_total_backlog,
      pool_total_windows
    )
  )

  if recovery_mode == "cold" and require_cold_recovery and not reached_cold then
    error(
      string.format(
        "cleanup did not reach cold within %d ms; last_diagnostics=%s",
        cold_wait_timeout_ms,
        last_diagnostics.raw
      )
    )
  end

  return {
    elapsed_ms = elapsed_ms,
    reached_cold = reached_cold,
    timed_out = timed_out,
    diagnostics = last_diagnostics,
  }
end

local function main()
  prepend_runtimepath(getenv_string("SMEAR_CURSOR_RTP", ""))
  prepend_package_cpath(getenv_string("SMEAR_CURSOR_CPATH", ""))

  local loaded_module_path = package.searchpath("rs_smear_cursor", package.cpath)
  if loaded_module_path == nil then
    error("failed to locate rs_smear_cursor in package.cpath")
  end
  -- Surprising: plain `require("rs_smear_cursor")` can still resolve an older installed module
  -- body even after the harness prepends the local release artifact to `package.cpath`.
  -- `package.loadlib` forces the harness to execute the exact dylib it is about to benchmark.
  local module_loader, load_error =
    package.loadlib(loaded_module_path, "luaopen_rs_smear_cursor")
  if module_loader == nil then
    error("failed to load rs_smear_cursor: " .. tostring(load_error))
  end
  local ok, smear = pcall(module_loader)
  if not ok then
    error("failed to initialize rs_smear_cursor: " .. tostring(smear))
  end
  package.loaded.rs_smear_cursor = smear
  local scenario_name = getenv_string("SMEAR_SCENARIO_NAME", "adhoc")

  local windows_count = getenv_positive_integer("SMEAR_WINDOWS", 8)
  local warmup_iterations = getenv_positive_integer("SMEAR_WARMUP_ITERATIONS", 500)
  local baseline_iterations = getenv_positive_integer("SMEAR_BASELINE_ITERATIONS", 3000)
  local stress_iterations = getenv_positive_integer("SMEAR_STRESS_ITERATIONS", 20000)
  local stress_rounds = getenv_positive_integer("SMEAR_STRESS_ROUNDS", 4)
  local recovery_iterations = getenv_positive_integer("SMEAR_RECOVERY_ITERATIONS", baseline_iterations)
  local recovery_mode = validate_recovery_mode(getenv_string("SMEAR_RECOVERY_MODE", "cold"))
  local settle_wait_ms = getenv_non_negative_number("SMEAR_SETTLE_WAIT_MS", 1200)
  local cold_wait_timeout_ms = getenv_positive_integer("SMEAR_COLD_WAIT_TIMEOUT_MS", 2500)
  local require_cold_recovery = getenv_bool("SMEAR_REQUIRE_COLD_RECOVERY", false)
  local recovery_poll_ms = getenv_positive_integer("SMEAR_RECOVERY_POLL_MS", 10)
  local max_recovery_ratio = getenv_non_negative_number("SMEAR_MAX_RECOVERY_RATIO", 1.4)
  local max_stress_ratio = getenv_non_negative_number("SMEAR_MAX_STRESS_RATIO", 2.0)
  local max_floating_windows = getenv_positive_integer("SMEAR_MAX_FLOATING_WINDOWS", 256)
  local smear_between_buffers = getenv_bool("SMEAR_BETWEEN_BUFFERS", false)
  local particles_enabled = getenv_bool("SMEAR_PARTICLES_ENABLED", false)
  local particles_over_text = getenv_bool("SMEAR_PARTICLES_OVER_TEXT", false)
  local logging_level = getenv_non_negative_integer("SMEAR_LOGGING_LEVEL", 4)
  local drain_every = getenv_non_negative_integer("SMEAR_DRAIN_EVERY", 16)
  local delay_event_to_smear = getenv_non_negative_number("SMEAR_DELAY_EVENT_TO_SMEAR", 1)
  local workload_line_count = getenv_positive_integer("SMEAR_LINE_COUNT", 2000)

  smear.setup({
    -- `logging_level = 4` is intentionally the quietest setting in this plugin, not the most
    -- verbose one. Keep perf runs on that setting unless the harness is explicitly debugging.
    logging_level = logging_level,
    particles_enabled = particles_enabled,
    particles_over_text = particles_over_text,
    smear_between_buffers = smear_between_buffers,
    smear_between_neighbor_lines = true,
    delay_event_to_smear = delay_event_to_smear,
    fps = 120,
  })

  emit_line(string.format("PERF_SCENARIO name=%s", scenario_name))
  emit_line(string.format("PERF_LIBRARY module_path=%s", loaded_module_path))

  local workload_buffer = create_workload_buffer(workload_line_count)
  local windows = create_split_windows(workload_buffer, windows_count)
  if #windows < 2 then
    error("need at least 2 windows for this harness")
  end

  emit_line(
    string.format(
      "PERF_CONFIG windows=%d workload_line_count=%d warmup_iterations=%d baseline_iterations=%d stress_iterations=%d stress_rounds=%d recovery_iterations=%d recovery_mode=%s settle_wait_ms=%.0f cold_wait_timeout_ms=%d require_cold_recovery=%s recovery_poll_ms=%d logging_level=%d smear_between_buffers=%s particles_enabled=%s particles_over_text=%s max_recovery_ratio=%.3f max_stress_ratio=%.3f drain_every=%d delay_event_to_smear=%.3f",
      #windows,
      workload_line_count,
      warmup_iterations,
      baseline_iterations,
      stress_iterations,
      stress_rounds,
      recovery_iterations,
      recovery_mode,
      settle_wait_ms,
      cold_wait_timeout_ms,
      tostring(require_cold_recovery),
      recovery_poll_ms,
      logging_level,
      tostring(smear_between_buffers),
      tostring(particles_enabled),
      tostring(particles_over_text),
      max_recovery_ratio,
      max_stress_ratio,
      drain_every,
      delay_event_to_smear
    )
  )

  run_switch_phase("warmup", windows, warmup_iterations, drain_every)
  local baseline = run_switch_phase("baseline", windows, baseline_iterations, drain_every)
  print_diagnostics("post_baseline", smear)

  local stress_results = {}
  for round = 1, stress_rounds do
    local result = run_switch_phase(
      string.format("stress_%d", round),
      windows,
      stress_iterations,
      drain_every
    )
    stress_results[#stress_results + 1] = result
  end

  local recovery_wait = wait_for_recovery(
    smear,
    recovery_mode,
    settle_wait_ms,
    cold_wait_timeout_ms,
    recovery_poll_ms,
    require_cold_recovery
  )

  local post_wait_floating_windows = count_floating_windows(false)
  local post_wait_visible_floating_windows = count_floating_windows(true)
  local post_wait_smear_floating_windows = count_smear_floating_windows(false)
  local post_wait_visible_smear_floating_windows = count_smear_floating_windows(true)
  emit_line(
    string.format(
      "PERF_RECOVERY_STATE cleanup_thermal=%s compaction_target_reached=%s queue_total_backlog=%s delayed_ingress_pending=%s",
      recovery_wait.diagnostics.fields.cleanup_thermal or "unknown",
      recovery_wait.diagnostics.fields.compaction_target_reached or "unknown",
      recovery_wait.diagnostics.fields.queue_total_backlog or "unknown",
      recovery_wait.diagnostics.fields.delayed_ingress_pending or "unknown"
    )
  )
  emit_line(
    string.format(
      "PERF_WINDOW_COUNTS phase=post_recovery_wait floating_windows=%d visible_floating_windows=%d smear_floating_windows=%d visible_smear_floating_windows=%d",
      post_wait_floating_windows,
      post_wait_visible_floating_windows,
      post_wait_smear_floating_windows,
      post_wait_visible_smear_floating_windows
    )
  )
  print_diagnostics("post_recovery_wait", smear)
  local recovery = run_switch_phase("recovery", windows, recovery_iterations, drain_every)
  print_diagnostics("post_recovery", smear)
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

  emit_line(
    string.format(
      "PERF_STRESS_SUMMARY max_avg_us=%.3f tail_avg_us=%.3f max_ratio=%.3f tail_ratio=%.3f",
      stress_max_avg_us,
      stress_tail_avg_us,
      stress_max_ratio,
      stress_tail_ratio
    )
  )

  emit_line(
    string.format(
      "PERF_SUMMARY baseline_avg_us=%.3f recovery_avg_us=%.3f recovery_ratio=%.3f recovery_wait_mode=%s recovery_wait_elapsed_ms=%.3f recovery_reached_cold=%s recovery_timed_out=%s post_wait_floating_windows=%d post_wait_visible_floating_windows=%d post_wait_smear_floating_windows=%d post_wait_visible_smear_floating_windows=%d",
      baseline.avg_us,
      recovery.avg_us,
      recovery_ratio,
      recovery_mode,
      recovery_wait.elapsed_ms,
      tostring(recovery_wait.reached_cold),
      tostring(recovery_wait.timed_out),
      post_wait_floating_windows,
      post_wait_visible_floating_windows,
      post_wait_smear_floating_windows,
      post_wait_visible_smear_floating_windows
    )
  )

  if post_wait_visible_floating_windows > max_floating_windows then
    error(
      string.format(
        "visible floating windows still high after recovery wait: expected <= %d, got %d",
        max_floating_windows,
        post_wait_visible_floating_windows
      )
    )
  end

  if post_wait_visible_smear_floating_windows > max_floating_windows then
    error(
      string.format(
        "visible smear floating windows still high after recovery wait: expected <= %d, got %d",
        max_floating_windows,
        post_wait_visible_smear_floating_windows
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

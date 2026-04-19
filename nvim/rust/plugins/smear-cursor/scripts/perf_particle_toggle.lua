local uv = vim.uv or vim.loop
local PERF_JSON_SCHEMA = "particle-toggle"
local PERF_JSON_VERSION = 1

local TARGET_ANCHORS = {
  { row = 5.0, col = 8.0 },
  { row = 16.0, col = 42.0 },
  { row = 7.0, col = 76.0 },
  { row = 14.0, col = 24.0 },
}

local function emit_line(line)
  io.stdout:write(line .. "\n")
  io.stdout:flush()
end

local function emit_json(kind, payload)
  emit_line(vim.json.encode({
    schema = PERF_JSON_SCHEMA,
    version = PERF_JSON_VERSION,
    kind = kind,
    payload = payload,
  }):gsub("^", "PERF_JSON ", 1))
end

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

local function getenv_positive_number(name, default_value)
  local raw_value = getenv_string(name, nil)
  if raw_value == nil then
    return default_value
  end

  local parsed = tonumber(raw_value)
  if parsed == nil or parsed <= 0 then
    error(string.format("%s must be a positive number, got %q", name, raw_value))
  end

  return parsed
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

local function prepend_package_cpath(path)
  if path == "" then
    return
  end

  if not vim.startswith(package.cpath, path .. ";") and package.cpath ~= path then
    package.cpath = path .. ";" .. package.cpath
  end
end

local function point(row, col)
  return { row, col }
end

local function rect(anchor)
  return {
    point(anchor.row, anchor.col),
    point(anchor.row, anchor.col + 1.0),
    point(anchor.row + 1.0, anchor.col + 1.0),
    point(anchor.row + 1.0, anchor.col),
  }
end

local function zero_corners()
  return {
    point(0.0, 0.0),
    point(0.0, 0.0),
    point(0.0, 0.0),
    point(0.0, 0.0),
  }
end

local function clone_point(value)
  return point(value[1], value[2])
end

local function clone_corners(value)
  return {
    clone_point(value[1]),
    clone_point(value[2]),
    clone_point(value[3]),
    clone_point(value[4]),
  }
end

local function clone_particles(value)
  local particles = {}
  for index, particle in ipairs(value) do
    particles[index] = {
      position = clone_point(particle.position),
      velocity = clone_point(particle.velocity),
      lifetime = particle.lifetime,
    }
  end
  return particles
end

local function clone_elapsed_ms(value)
  return { value[1], value[2], value[3], value[4] }
end

local function same_corners(left, right)
  for index = 1, 4 do
    local left_corner = left[index]
    local right_corner = right[index]
    if left_corner[1] ~= right_corner[1] or left_corner[2] ~= right_corner[2] then
      return false
    end
  end
  return true
end

local function target_corners_for_iteration(iteration, retarget_interval)
  local phase = math.floor((iteration - 1) / retarget_interval) + 1
  local anchor_index = phase % #TARGET_ANCHORS + 1
  return rect(TARGET_ANCHORS[anchor_index])
end

local function build_step_input(particles_enabled, time_interval_ms, particle_max_num)
  local initial_current = rect(TARGET_ANCHORS[1])
  return {
    mode = "n",
    time_interval = time_interval_ms,
    config_time_interval = time_interval_ms,
    head_response_ms = 110.0,
    damping_ratio = 1.0,
    current_corners = clone_corners(initial_current),
    trail_origin_corners = clone_corners(initial_current),
    target_corners = rect(TARGET_ANCHORS[2]),
    spring_velocity_corners = zero_corners(),
    trail_elapsed_ms = { 0.0, 0.0, 0.0, 0.0 },
    max_length = 25.0,
    max_length_insert_mode = 25.0,
    trail_duration_ms = 150.0,
    trail_min_distance = 0.0,
    trail_thickness = 1.0,
    trail_thickness_x = 1.0,
    particles = {},
    previous_center = point(TARGET_ANCHORS[1].row + 0.5, TARGET_ANCHORS[1].col + 0.5),
    particle_damping = 0.2,
    particles_enabled = particles_enabled,
    particle_gravity = 20.0,
    particle_random_velocity = 100.0,
    particle_max_num = particle_max_num,
    particle_spread = 0.5,
    particles_per_second = 200.0,
    particles_per_length = 1.0,
    particle_max_initial_velocity = 10.0,
    particle_velocity_from_cursor = 0.2,
    particle_max_lifetime = 300.0,
    particle_lifetime_distribution_exponent = 5.0,
    min_distance_emit_particles = 1.5,
    vertical_bar = false,
    horizontal_bar = false,
    block_aspect_ratio = 2.0,
    rng_state = 0xA341316C,
  }
end

local function load_smear_module()
  prepend_package_cpath(getenv_string("SMEAR_CURSOR_CPATH", ""))

  local loaded_module_path = package.searchpath("nvimrs_smear_cursor", package.cpath)
  if loaded_module_path == nil then
    error("failed to locate nvimrs_smear_cursor in package.cpath")
  end

  local module_loader, load_error =
    package.loadlib(loaded_module_path, "luaopen_nvimrs_smear_cursor")
  if module_loader == nil then
    error("failed to load nvimrs_smear_cursor: " .. tostring(load_error))
  end

  local ok, smear = pcall(module_loader)
  if not ok then
    error("failed to initialize nvimrs_smear_cursor: " .. tostring(smear))
  end

  package.loaded.nvimrs_smear_cursor = smear
  return smear, loaded_module_path
end

local function run_iterations(smear, input, start_iteration, iterations, retarget_interval)
  local particle_sum = 0.0
  local max_particles = 0
  local final_particles = 0
  local retargets = 0
  local start_ns = uv.hrtime()

  for offset = 0, iterations - 1 do
    local iteration = start_iteration + offset
    local next_target = target_corners_for_iteration(iteration, retarget_interval)
    if not same_corners(next_target, input.target_corners) then
      input.trail_origin_corners = clone_corners(input.current_corners)
      input.target_corners = next_target
      retargets = retargets + 1
    end

    local output = smear.step(input)
    local particle_count = #output.particles
    particle_sum = particle_sum + particle_count
    if particle_count > max_particles then
      max_particles = particle_count
    end
    final_particles = particle_count

    input.current_corners = clone_corners(output.current_corners)
    input.spring_velocity_corners = clone_corners(output.spring_velocity_corners)
    input.trail_elapsed_ms = clone_elapsed_ms(output.trail_elapsed_ms)
    input.particles = clone_particles(output.particles)
    input.previous_center = clone_point(output.previous_center)
    input.rng_state = output.rng_state
  end

  local elapsed_ns = uv.hrtime() - start_ns
  return input, {
    avg_us = (elapsed_ns / iterations) / 1e3,
    avg_particles = particle_sum / iterations,
    max_particles = max_particles,
    final_particles = final_particles,
    retargets = retargets,
  }
end

local function main()
  local warmup_iterations = getenv_positive_integer("SMEAR_WARMUP_ITERATIONS", 600)
  local benchmark_iterations = getenv_positive_integer("SMEAR_BENCHMARK_ITERATIONS", 2400)
  local retarget_interval = getenv_positive_integer("SMEAR_RETARGET_INTERVAL", 24)
  local particles_enabled = getenv_bool("SMEAR_PARTICLES_ENABLED", false)
  local time_interval_ms = getenv_positive_number("SMEAR_TIME_INTERVAL_MS", 1000.0 / 120.0)
  local particle_max_num = getenv_positive_integer("SMEAR_PARTICLE_MAX_NUM", 100)
  local scenario_name = particles_enabled and "particles_on" or "particles_off"

  local smear, loaded_module_path = load_smear_module()
  local input = build_step_input(particles_enabled, time_interval_ms, particle_max_num)

  emit_line(string.format("PERF_SCENARIO name=%s", scenario_name))
  emit_json("scenario", { name = scenario_name })
  emit_line(string.format("PERF_LIBRARY module_path=%s", loaded_module_path))
  emit_json("library", { module_path = loaded_module_path })
  emit_line(
    string.format(
      "PERF_CONFIG warmup_iterations=%d benchmark_iterations=%d retarget_interval=%d particles_enabled=%s time_interval_ms=%.3f particle_max_num=%d anchor_count=%d",
      warmup_iterations,
      benchmark_iterations,
      retarget_interval,
      tostring(particles_enabled),
      time_interval_ms,
      particle_max_num,
      #TARGET_ANCHORS
    )
  )
  emit_json("config", {
    warmup_iterations = warmup_iterations,
    benchmark_iterations = benchmark_iterations,
    retarget_interval = retarget_interval,
    particles_enabled = particles_enabled,
    time_interval_ms = time_interval_ms,
    particle_max_num = particle_max_num,
    anchor_count = #TARGET_ANCHORS,
  })

  local bench_start_iteration = warmup_iterations + 1
  input = run_iterations(smear, input, 1, warmup_iterations, retarget_interval)
  local _, metrics = run_iterations(
    smear,
    input,
    bench_start_iteration,
    benchmark_iterations,
    retarget_interval
  )

  if not particles_enabled and metrics.max_particles ~= 0 then
    error(string.format("particles disabled run retained %d particles", metrics.max_particles))
  end

  emit_line(
    string.format(
      "PERF_SUMMARY avg_us=%.3f avg_particles=%.3f max_particles=%d final_particles=%d retargets=%d",
      metrics.avg_us,
      metrics.avg_particles,
      metrics.max_particles,
      metrics.final_particles,
      metrics.retargets
    )
  )
  emit_json("summary", {
    avg_us = metrics.avg_us,
    avg_particles = metrics.avg_particles,
    max_particles = metrics.max_particles,
    final_particles = metrics.final_particles,
    retargets = metrics.retargets,
  })
end

local ok, err = xpcall(main, debug.traceback)
if not ok then
  io.stderr:write(err .. "\n")
  io.stderr:flush()
  vim.cmd("cquit 1")
  return
end

vim.cmd("qa!")

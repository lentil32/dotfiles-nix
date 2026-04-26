local EXPECTED_HOST_BRIDGE_REVISION = 15

local function prepend_runtimepath(path)
  if path == nil or path == "" then
    return
  end
  vim.opt.runtimepath:prepend(path)
end

local function append_package_cpath(path)
  if path == nil or path == "" then
    return
  end
  package.cpath = path .. ";" .. package.cpath
end

local function assert_no_log_match(log_path, needles)
  if log_path == nil or log_path == "" or vim.fn.filereadable(log_path) ~= 1 then
    return
  end

  for _, line in ipairs(vim.fn.readfile(log_path)) do
    for _, needle in ipairs(needles) do
      if line:find(needle, 1, true) then
        error("unexpected smear cursor log line: " .. line)
      end
    end
  end
end

local function wait_for(description, predicate)
  if vim.wait(250, predicate, 5) then
    return
  end

  error(description)
end

local function matching_call_count(observed_calls, host_callback_id, host_timer_id)
  local count = 0
  for _, call in ipairs(observed_calls) do
    if call.host_callback_id == host_callback_id
      and (host_timer_id == nil or call.host_timer_id == host_timer_id)
    then
      count = count + 1
    end
  end
  return count
end

local function main()
  prepend_runtimepath(os.getenv("SMEAR_CURSOR_RTP") or "")
  append_package_cpath(os.getenv("SMEAR_CURSOR_CPATH") or "")

  local ok, smear = pcall(require, "nvimrs_smear_cursor")
  if not ok then
    error("failed to require nvimrs_smear_cursor: " .. tostring(smear))
  end

  smear.setup({
    enabled = true,
    logging_level = 0,
    delay_event_to_smear = 0,
    time_interval = 1000.0 / 120.0,
  })
  vim.wait(50, function()
    return false
  end, 5)

  local revision = vim.fn["nvimrs_smear_cursor#host_bridge#revision"]()
  if revision ~= EXPECTED_HOST_BRIDGE_REVISION then
    error("unexpected host bridge revision: " .. tostring(revision))
  end

  local observed_timer_callbacks = {}
  local original_on_core_timer_fired = smear.on_core_timer_fired
  smear.on_core_timer_fired = function(host_callback_id, host_timer_id)
    table.insert(observed_timer_callbacks, {
      host_callback_id = host_callback_id,
      host_timer_id = host_timer_id,
    })
    return original_on_core_timer_fired(host_callback_id, host_timer_id)
  end

  local observed_autocmd_payloads = {}
  local original_on_autocmd_payload = smear.on_autocmd_payload
  smear.on_autocmd_payload = function(payload)
    table.insert(observed_autocmd_payloads, {
      event = payload.event,
      buffer = payload.buffer,
      match = payload.match,
    })
    return original_on_autocmd_payload(payload)
  end

  if package.loaded["nvimrs_smear_cursor.probes"] == nil then
    error("probe helpers were not installed during setup")
  end

  local autocmd_dispatch_result = vim.fn["nvimrs_smear_cursor#host_bridge#dispatch_autocmd"](
    "OptionSet",
    0,
    ""
  )
  if autocmd_dispatch_result ~= 0 then
    error("autocmd bridge returned an unexpected status")
  end
  if #observed_autocmd_payloads ~= 1 then
    error("autocmd bridge did not route exactly one payload")
  end
  local observed_autocmd_payload = observed_autocmd_payloads[1]
  if observed_autocmd_payload.event ~= "OptionSet" then
    error("autocmd bridge forwarded an unexpected event name")
  end
  if observed_autocmd_payload.buffer ~= 0 then
    error("autocmd bridge forwarded an unexpected buffer handle")
  end
  if observed_autocmd_payload.match ~= "" then
    error("autocmd bridge forwarded an unexpected match name")
  end

  local cursor_color = vim.fn["nvimrs_smear_cursor#host_bridge#cursor_color_at_cursor"](false)
  if cursor_color ~= nil and cursor_color ~= vim.NIL then
    local cursor_color_type = type(cursor_color)
    if cursor_color_type ~= "table" then
      error("cursor color probe returned unexpected type: " .. cursor_color_type)
    end

    local color = cursor_color.color
    if color ~= nil and type(color) ~= "number" then
      error("cursor color probe returned non-numeric color field")
    end
    if type(cursor_color.used_extmark_fallback) ~= "boolean" then
      error("cursor color probe returned non-boolean used_extmark_fallback field")
    end
  end

  local allowed_mask = vim.fn["nvimrs_smear_cursor#host_bridge#background_allowed_mask"]({
    1,
    1,
    1,
    0x2800,
    0x28FF,
    0x1CD00,
    0x1CDE7,
  })
  if type(allowed_mask) ~= "table" or #allowed_mask ~= 1 or type(allowed_mask[1]) ~= "number" then
    error("background mask probe returned unexpected shape")
  end

  local stopped_timer_callback_id = 21
  local stopped_timer_id =
    vim.fn["nvimrs_smear_cursor#host_bridge#start_timer_once"](stopped_timer_callback_id, 60)
  vim.fn["nvimrs_smear_cursor#host_bridge#stop_timer"](stopped_timer_id)
  vim.wait(120, function()
    return false
  end, 5)
  if matching_call_count(observed_timer_callbacks, stopped_timer_callback_id) ~= 0 then
    error("stopped timer still delivered a callback")
  end

  local slow_timer_callback_id = 31
  local fast_timer_callback_id = 32
  local slow_timer_id = vim.fn["nvimrs_smear_cursor#host_bridge#start_timer_once"](slow_timer_callback_id, 80)
  local fast_timer_id = vim.fn["nvimrs_smear_cursor#host_bridge#start_timer_once"](fast_timer_callback_id, 0)
  if type(slow_timer_id) ~= "number" or type(fast_timer_id) ~= "number" then
    error("timer bridge did not return numeric timer ids")
  end
  if slow_timer_id <= 0 or fast_timer_id <= 0 then
    error("timer bridge returned a non-positive timer id")
  end
  if slow_timer_id == fast_timer_id then
    error("timer bridge unexpectedly reused a timer id")
  end
  wait_for("parallel timer bridge did not deliver the immediate callback", function()
    return matching_call_count(observed_timer_callbacks, fast_timer_callback_id, fast_timer_id) == 1
  end)
  vim.wait(120, function()
    return false
  end, 5)
  if matching_call_count(observed_timer_callbacks, slow_timer_callback_id) ~= 1 then
    error("stateless timer bridge did not preserve both callbacks")
  end
  if matching_call_count(observed_timer_callbacks, slow_timer_callback_id, slow_timer_id) ~= 1 then
    error("stateless timer bridge did not round-trip the earlier host timer id")
  end
  if matching_call_count(observed_timer_callbacks, fast_timer_callback_id, fast_timer_id) ~= 1 then
    error("stateless timer bridge did not round-trip the immediate host timer id")
  end

  assert_no_log_match(os.getenv("SMEAR_CURSOR_LOG_FILE"), {
    "failed to schedule core timer",
    "core timer callback received invalid host timer payload",
    "scheduled callback panicked",
  })

  print("SMEAR_TIMER_BRIDGE_OK")
end

local ok, err = pcall(main)
if not ok then
  error(err)
end

vim.cmd("qa!")

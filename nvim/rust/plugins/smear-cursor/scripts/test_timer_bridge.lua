local EXPECTED_HOST_BRIDGE_REVISION = 9

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

local function matching_call_count(observed_calls, slot_id, generation)
  local count = 0
  for _, call in ipairs(observed_calls) do
    if call.slot_id == slot_id and (generation == nil or call.generation == generation) then
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

  if vim.fn.exists("*nvimrs_smear_cursor#host_bridge#start_timer_once") ~= 1 then
    error("missing nvimrs_smear_cursor#host_bridge#start_timer_once bridge")
  end

  if vim.fn.exists("*nvimrs_smear_cursor#host_bridge#stop_timer") ~= 1 then
    error("missing nvimrs_smear_cursor#host_bridge#stop_timer bridge")
  end

  if vim.fn.exists("*nvimrs_smear_cursor#host_bridge#install_probe_helpers") ~= 1 then
    error("missing nvimrs_smear_cursor#host_bridge#install_probe_helpers bridge")
  end

  if vim.fn.exists("*nvimrs_smear_cursor#host_bridge#cursor_color_at_cursor") ~= 1 then
    error("missing nvimrs_smear_cursor#host_bridge#cursor_color_at_cursor bridge")
  end

  if vim.fn.exists("*nvimrs_smear_cursor#host_bridge#background_allowed_mask") ~= 1 then
    error("missing nvimrs_smear_cursor#host_bridge#background_allowed_mask bridge")
  end

  if type(smear.on_core_timer_slot) ~= "function" then
    error("missing nvimrs_smear_cursor.on_core_timer_slot bridge")
  end

  local observed_timer_callbacks = {}
  local original_on_core_timer_slot = smear.on_core_timer_slot
  smear.on_core_timer_slot = function(slot_id, generation)
    table.insert(observed_timer_callbacks, {
      slot_id = slot_id,
      generation = generation,
    })
    return original_on_core_timer_slot(slot_id, generation)
  end

  if package.loaded["nvimrs_smear_cursor.probes"] == nil then
    error("probe helpers were not installed during setup")
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

  vim.fn["nvimrs_smear_cursor#host_bridge#start_timer_once"](97, 11, 0)
  wait_for("timer bridge did not deliver the immediate callback", function()
    return matching_call_count(observed_timer_callbacks, 97, 11) == 1
  end)
  if matching_call_count(observed_timer_callbacks, 97) ~= 1 then
    error("immediate timer callback fired more than once")
  end

  vim.fn["nvimrs_smear_cursor#host_bridge#start_timer_once"](98, 21, 60)
  vim.fn["nvimrs_smear_cursor#host_bridge#stop_timer"](98)
  vim.wait(120, function()
    return false
  end, 5)
  if matching_call_count(observed_timer_callbacks, 98) ~= 0 then
    error("stopped timer slot still delivered a callback")
  end

  vim.fn["nvimrs_smear_cursor#host_bridge#start_timer_once"](99, 31, 80)
  vim.fn["nvimrs_smear_cursor#host_bridge#start_timer_once"](99, 32, 0)
  wait_for("re-armed timer bridge did not deliver the replacement callback", function()
    return matching_call_count(observed_timer_callbacks, 99, 32) == 1
  end)
  vim.wait(120, function()
    return false
  end, 5)
  if matching_call_count(observed_timer_callbacks, 99) ~= 1 then
    error("re-armed timer slot fired an unexpected number of callbacks")
  end
  if matching_call_count(observed_timer_callbacks, 99, 31) ~= 0 then
    error("re-armed timer slot still fired the canceled generation")
  end

  assert_no_log_match(os.getenv("SMEAR_CURSOR_LOG_FILE"), {
    "failed to schedule core timer",
    "Unknown function: v:lua.__nvimrs_smear_cursor_start_timer_once",
    "core timer callback received invalid timer id",
    "scheduled callback panicked",
  })

  print("SMEAR_TIMER_BRIDGE_OK")
end

local ok, err = pcall(main)
if not ok then
  error(err)
end

vim.cmd("qa!")

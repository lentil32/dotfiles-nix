local M = {}

local uv = vim.uv or vim.loop
local timer_slots = {}

local function validate_timer_slot(slot_id)
  if type(slot_id) ~= "number" or slot_id % 1 ~= 0 or slot_id <= 0 then
    error("invalid smear cursor timer slot: " .. tostring(slot_id))
  end

  return slot_id
end

local function validate_token_generation(token_generation)
  if type(token_generation) ~= "number" or token_generation % 1 ~= 0 or token_generation < 0 then
    error("invalid smear cursor timer generation: " .. tostring(token_generation))
  end

  return token_generation
end

local function normalized_timeout(timeout)
  if type(timeout) ~= "number" or timeout ~= timeout then
    return 0
  end

  if timeout < 0 then
    return 0
  end

  return timeout
end

local function timer_slot(slot_id)
  local slot = timer_slots[slot_id]
  if slot ~= nil then
    return slot
  end

  local handle = uv.new_timer()
  if handle == nil then
    error("failed to allocate smear cursor host timer for slot " .. tostring(slot_id))
  end

  slot = { handle = handle }
  timer_slots[slot_id] = slot
  return slot
end

local function dispatch_core_timer(slot_id, token_generation)
  require("nvimrs_smear_cursor").on_core_timer_slot(slot_id, token_generation)
end

function M.start_timer_once(slot_id, token_generation, timeout)
  slot_id = validate_timer_slot(slot_id)
  token_generation = validate_token_generation(token_generation)
  timeout = normalized_timeout(timeout)

  local slot = timer_slot(slot_id)
  slot.handle:stop()
  slot.handle:start(timeout, 0, function()
    vim.schedule(function()
      dispatch_core_timer(slot_id, token_generation)
    end)
  end)

  return slot_id
end

function M.stop_timer(slot_id)
  slot_id = validate_timer_slot(slot_id)

  local slot = timer_slots[slot_id]
  if slot == nil then
    return 0
  end

  slot.handle:stop()
  return 1
end

return M

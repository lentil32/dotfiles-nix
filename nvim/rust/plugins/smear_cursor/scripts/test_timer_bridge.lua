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

local function main()
  prepend_runtimepath(os.getenv("SMEAR_CURSOR_RTP") or "")
  append_package_cpath(os.getenv("SMEAR_CURSOR_CPATH") or "")

  local ok, smear = pcall(require, "rs_smear_cursor")
  if not ok then
    error("failed to require rs_smear_cursor: " .. tostring(smear))
  end

  smear.setup({
    enabled = true,
    logging_level = 0,
    delay_after_key = 0,
    delay_event_to_smear = 0,
    fps = 120,
  })

  smear.on_key()
  vim.wait(50, function()
    return false
  end, 5)

  local revision = vim.fn["rs_smear_cursor#host_bridge#revision"]()
  if revision ~= 2 then
    error("unexpected host bridge revision: " .. tostring(revision))
  end

  if vim.fn.exists("*rs_smear_cursor#host_bridge#on_core_timer") ~= 1 then
    error("missing rs_smear_cursor#host_bridge#on_core_timer bridge")
  end

  if vim.fn.exists("*rs_smear_cursor#host_bridge#start_timer_once") ~= 1 then
    error("missing rs_smear_cursor#host_bridge#start_timer_once bridge")
  end

  if vim.fn.exists("*rs_smear_cursor#host_bridge#set_on_key_listener") ~= 0 then
    error("legacy rs_smear_cursor#host_bridge#set_on_key_listener bridge still installed")
  end

  if vim.fn.luaeval("_G.__rs_smear_cursor_on_core_timer ~= nil") then
    error("legacy Lua timer callback global unexpectedly installed")
  end

  assert_no_log_match(os.getenv("SMEAR_CURSOR_LOG_FILE"), {
    "failed to schedule core timer",
    "Unknown function: v:lua.__rs_smear_cursor_start_timer_once",
  })

  print("SMEAR_TIMER_BRIDGE_OK")
end

local ok, err = pcall(main)
if not ok then
  error(err)
end

vim.cmd("qa!")

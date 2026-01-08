local util = require("config.util")

local M = {}

local function get_sidekick()
  return util.try_require("sidekick")
end

local function call_cli(method, opts)
  local cli = util.try_require("sidekick.cli")
  if cli and cli[method] then
    cli[method](opts)
  end
end

function M.nes_jump_or_apply()
  local sidekick = get_sidekick()
  if not sidekick then
    return false
  end
  return sidekick.nes_jump_or_apply()
end

function M.setup()
  local sidekick = get_sidekick()
  if not sidekick then
    return
  end

  ---@type sidekick.Config
  local sidekick_opts = {}
  sidekick.setup(sidekick_opts)

  local Snacks = _G.Snacks or require("snacks")

  Snacks.keymap.set("n", "<tab>", function()
    -- if there is a next edit, jump to it, otherwise apply it if any
    if not require("sidekick").nes_jump_or_apply() then
      return "<Tab>" -- fallback to normal tab
    end
    return ""
  end, { expr = true, desc = "Goto/Apply Next Edit Suggestion" })

  Snacks.keymap.set({ "n", "t", "i", "x" }, "<c-.>", function()
    call_cli("toggle")
  end, { desc = "Sidekick toggle" })

  Snacks.keymap.set("n", "<leader>aa", function()
    call_cli("toggle")
  end, { desc = "Sidekick toggle CLI" })

  Snacks.keymap.set("n", "<leader>as", function()
    call_cli("select")
  end, { desc = "Sidekick select CLI" })

  Snacks.keymap.set("n", "<leader>ad", function()
    call_cli("close")
  end, { desc = "Sidekick detach CLI session" })

  Snacks.keymap.set({ "n", "x" }, "<leader>at", function()
    call_cli("send", { msg = "{this}" })
  end, { desc = "Sidekick send this" })

  Snacks.keymap.set("n", "<leader>af", function()
    call_cli("send", { msg = "{file}" })
  end, { desc = "Sidekick send file" })

  Snacks.keymap.set("x", "<leader>av", function()
    call_cli("send", { msg = "{selection}" })
  end, { desc = "Sidekick send selection" })

  Snacks.keymap.set({ "n", "x" }, "<leader>ap", function()
    call_cli("prompt")
  end, { desc = "Sidekick prompt" })

  Snacks.keymap.set("n", "<leader>ac", function()
    require("sidekick.cli").toggle({ name = "codex", focus = true })
  end, { desc = "Sidekick Toggle Claude" })
end

return M

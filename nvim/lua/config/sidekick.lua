local M = {}

local function get_sidekick()
  local ok, sidekick = pcall(require, "sidekick")
  if not ok then
    return nil
  end
  return sidekick
end

local function call_cli(method, opts)
  local ok, cli = pcall(require, "sidekick.cli")
  if not ok then
    return
  end
  cli[method](opts)
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

  vim.keymap.set("<tab>", function()
    -- if there is a next edit, jump to it, otherwise apply it if any
    if not require("sidekick").nes_jump_or_apply() then
      return "<Tab>" -- fallback to normal tab
    end
  end, { expr = true, desc = "Goto/Apply Next Edit Suggestion" })

  vim.keymap.set({ "n", "t", "i", "x" }, "<c-.>", function()
    call_cli("toggle")
  end, { desc = "Sidekick toggle" })

  vim.keymap.set("<leader>aa", function()
    call_cli("toggle")
  end, { desc = "Sidekick toggle CLI" })

  vim.keymap.set("<leader>as", function()
    call_cli("select")
  end, { desc = "Sidekick select CLI" })

  vim.keymap.set("<leader>ad", function()
    call_cli("close")
  end, { desc = "Sidekick detach CLI session" })

  vim.keymap.set({ "n", "x" }, "<leader>at", function()
    call_cli("send", { msg = "{this}" })
  end, { desc = "Sidekick send this" })

  vim.keymap.set("<leader>af", function()
    call_cli("send", { msg = "{file}" })
  end, { desc = "Sidekick send file" })

  vim.keymap.set("x", "<leader>av", function()
    call_cli("send", { msg = "{selection}" })
  end, { desc = "Sidekick send selection" })

  vim.keymap.set({ "n", "x" }, "<leader>ap", function()
    call_cli("prompt")
  end, { desc = "Sidekick prompt" })

  vim.keymap.set("<leader>ac", function()
    require("sidekick.cli").toggle({ name = "codex", focus = true })
  end, { desc = "Sidekick Toggle Claude" })
end

return M

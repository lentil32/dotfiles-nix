local colorscheme = require("myLuaConf.colorscheme")
local keymaps = require("myLuaConf.keymaps")
local snacks = require("myLuaConf.snacks")
local util = require("myLuaConf.util")

colorscheme.apply()
snacks.setup()
require("myLuaConf.project").setup()
require("myLuaConf.oil").setup()
require("myLuaConf.autocmds").setup()
keymaps.setup()

require("lze").load({
  {
    "which-key.nvim",
    for_cat = "general",
    event = "DeferredUIEnter",
    after = function()
      local wk = require("which-key")
      wk.setup({ delay = 300 })
      wk.add(keymaps.list())
    end,
  },
  {
    "orgmode",
    for_cat = "org",
    ft = "org",
    after = function()
      require("myLuaConf.org").setup()
    end,
  },
  {
    "sidekick.nvim",
    for_cat = "general",
    event = "BufReadPre",
    after = function()
      require("sidekick").setup({
        cli = {
          win = {
            keys = {
              close_wx = { "<leader>wx", "close", mode = "nt", desc = "Close Sidekick" },
            },
          },
        },
      })

      local Snacks = _G.Snacks or require("snacks")
      local function call_cli(method, opts)
        local cli = require("sidekick.cli")
        if cli[method] then
          cli[method](opts)
        end
      end

      Snacks.keymap.set("n", "<tab>", function()
        if not require("sidekick").nes_jump_or_apply() then
          return "<Tab>"
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
    end,
  },
  {
    "smear-cursor.nvim",
    for_cat = "general",
    event = "DeferredUIEnter",
    after = function()
      if util.get_var(nil, "neovide") then
        return
      end
      require("smear_cursor").setup({})
    end,
  },
  { import = "myLuaConf.plugins.completion" },
  { import = "myLuaConf.plugins.motion" },
  { import = "myLuaConf.plugins.grug_far" },
  { import = "myLuaConf.plugins.git" },
  { import = "myLuaConf.plugins.syntax" },
})

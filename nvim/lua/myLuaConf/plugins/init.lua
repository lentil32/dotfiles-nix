local colorscheme = require("myLuaConf.colorscheme")
local keymaps = require("myLuaConf.keymaps")
local project = require("myLuaConf.project")
local snacks = require("myLuaConf.snacks")
local util = require("myLuaConf.util")

colorscheme.apply()
snacks.setup()
project.setup()
require("myLuaConf.oil").setup()
require("myLuaConf.autocmds").setup()
keymaps.setup()

local function sidekick_in_project_root(fn)
  return function(...)
    local root = project.project_root_or_warn()
    if root then
      vim.cmd("lcd " .. vim.fn.fnameescape(root))
    end
    return fn(...)
  end
end

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
    on_require = { "orgmode" },
    keys = {
      { "<leader>aoa", require("myLuaConf.org").action("agenda.prompt"), desc = "Agenda" },
      { "<leader>aoc", require("myLuaConf.org").action("capture.prompt"), desc = "Capture" },
    },
    after = function()
      require("myLuaConf.org").setup()
    end,
  },
  {
    "sidekick.nvim",
    for_cat = "general",
    event = "BufReadPre",
    keys = {
      {
        "<tab>",
        "v:lua.require('sidekick').nes_jump_or_apply() and '' or '<Tab>'",
        mode = "n",
        expr = true,
        desc = "Goto/Apply Next Edit Suggestion",
      },
      {
        "<c-.>",
        sidekick_in_project_root(function()
          require("sidekick.cli").toggle()
        end),
        mode = { "n", "t", "i", "x" },
        desc = "Sidekick toggle",
      },
      {
        "<leader>aa",
        sidekick_in_project_root(function()
          require("sidekick.cli").toggle()
        end),
        mode = "n",
        desc = "Sidekick toggle CLI",
      },
      {
        "<leader>as",
        sidekick_in_project_root(function()
          require("sidekick.cli").select()
        end),
        mode = "n",
        desc = "Sidekick select CLI",
      },
      {
        "<leader>ad",
        function()
          require("sidekick.cli").close()
        end,
        mode = "n",
        desc = "Sidekick detach CLI session",
      },
      {
        "<leader>at",
        function()
          require("sidekick.cli").send({ msg = "{this}" })
        end,
        mode = { "n", "x" },
        desc = "Sidekick send this",
      },
      {
        "<leader>af",
        function()
          require("sidekick.cli").send({ msg = "{file}" })
        end,
        mode = "n",
        desc = "Sidekick send file",
      },
      {
        "<leader>av",
        function()
          require("sidekick.cli").send({ msg = "{selection}" })
        end,
        mode = "x",
        desc = "Sidekick send selection",
      },
      {
        "<leader>ap",
        function()
          require("sidekick.cli").prompt()
        end,
        mode = { "n", "x" },
        desc = "Sidekick prompt",
      },
      {
        "<leader>ac",
        sidekick_in_project_root(function()
          require("sidekick.cli").toggle({ name = "codex", focus = true })
        end),
        mode = "n",
        desc = "Sidekick Toggle Claude",
      },
    },
    after = function()
      require("sidekick").setup({
        cli = {
          win = {
            keys = {
              close_wx = { "<leader>wx", "close", mode = "n", desc = "Close Sidekick" },
              prompt = false,
            },
          },
        },
      })
    end,
  },
  {
    "nvim-autopairs",
    for_cat = "general",
    event = "InsertEnter",
    after = function()
      require("nvim-autopairs").setup({
        check_ts = true,
      })
    end,
  },
  {
    "overseer.nvim",
    for_cat = "general",
    event = "DeferredUIEnter",
    cmd = {
      "OverseerOpen",
      "OverseerClose",
      "OverseerToggle",
      "OverseerRun",
      "OverseerShell",
      "OverseerTaskAction",
    },
    after = function()
      require("overseer").setup({})
    end,
  },
  { import = "myLuaConf.plugins.statusline" },
  { import = "myLuaConf.plugins.completion" },
  { import = "myLuaConf.plugins.motion" },
  { import = "myLuaConf.plugins.grug_far" },
  { import = "myLuaConf.plugins.git" },
  { import = "myLuaConf.plugins.syntax" },
})

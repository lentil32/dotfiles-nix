local colorscheme = require("myLuaConf.colorscheme")
local keymaps = require("myLuaConf.keymaps")
local plugin_util = require("rs_plugin_util")
local project = require("myLuaConf.project")
local snacks = require("myLuaConf.snacks")
colorscheme.apply()
snacks.setup()
project.setup()
require("myLuaConf.oil").setup()
require("rs_autocmds").setup()
keymaps.setup()

local function should_enable_smear_cursor()
  if plugin_util.get_var(nil, "neovide") then
    return false
  end
  local term_program = vim.env.TERM_PROGRAM
  if term_program and term_program == "ghostty" then
    return false
  end
  return true
end

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
      wk.setup({
        delay = 300,
        -- Avoid auto-triggers (g/z/[...]) and ModeChanged popups in x/o modes.
        -- Keep which-key focused on leader/localleader which is what we use it for.
        triggers = {
          { "<leader>",      mode = { "n", "x" } },
          { "<localleader>", mode = { "n", "x" } },
        },
        spec = keymaps.list(),
      })
    end,
  },
  {
    "orgmode",
    for_cat = "org",
    ft = "org",
    on_require = { "orgmode" },
    keys = {
      { "<leader>aoa", require("myLuaConf.org").action("agenda.prompt"),  desc = "Agenda" },
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
        "<c-.>",
        sidekick_in_project_root(function()
          require("sidekick.cli").toggle({ name = "codex", focus = true })
          vim.cmd.stopinsert()
        end),
        mode = { "n", "t", "i", "x" },
        desc = "Sidekick toggle Codex",
      },
      {
        "<leader>ca",
        sidekick_in_project_root(function()
          require("sidekick.cli").toggle()
        end),
        mode = "n",
        desc = "Sidekick toggle CLI",
      },
      {
        "<leader>cs",
        sidekick_in_project_root(function()
          require("sidekick.cli").select()
        end),
        mode = "n",
        desc = "Sidekick select CLI",
      },
      {
        "<leader>cd",
        function()
          require("sidekick.cli").close()
        end,
        mode = "n",
        desc = "Sidekick detach CLI session",
      },
      {
        "<leader>ct",
        function()
          require("sidekick.cli").send({ msg = "{this}" })
        end,
        mode = { "n", "x" },
        desc = "Sidekick send this",
      },
      {
        "<leader>cf",
        function()
          require("sidekick.cli").send({ msg = "{file}" })
        end,
        mode = "n",
        desc = "Sidekick send file",
      },
      {
        "<leader>cv",
        function()
          require("sidekick.cli").send({ msg = "{selection}" })
        end,
        mode = "x",
        desc = "Sidekick send selection",
      },
      {
        "<leader>cp",
        function()
          require("sidekick.cli").prompt()
        end,
        mode = { "n", "x" },
        desc = "Sidekick prompt",
      },
      {
        "<leader>cc",
        sidekick_in_project_root(function()
          require("sidekick.cli").toggle({ name = "codex", focus = true })
        end),
        mode = "n",
        desc = "Sidekick toggle Codex",
      },
    },
    after = function()
      require("sidekick").setup({
        cli = {
          tools = {
            codex = {
              cmd = { "mise", "exec", "--", "codex" },
            },
          },
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
  -- {
  --   "rs-smear-cursor",
  --   for_cat = "general",
  --   event = "DeferredUIEnter",
  --   -- Already available via startupPlugins; no packadd needed.
  --   load = function(_) end,
  --   after = function()
  --     if not should_enable_smear_cursor() then
  --       return
  --     end
  --     require("rs_smear_cursor").setup({
  --       smear_terminal_mode = true,
  --       filetypes_disabled = {
  --         "snacks_picker_preview",
  --         "sidekick_terminal"
  --       },
  --     })
  --   end,
  -- },
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

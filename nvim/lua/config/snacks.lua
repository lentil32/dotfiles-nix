local oil = require("config.oil")
local org = require("config.org")
local preview = require("config.snacks_preview")

local M = {}

function M.setup()
  ---@type snacks.Config
  local snacks_opts = {
    styles = {
      dashboard = {
        -- Avoid double BufDelete/BufWipeout callbacks in snacks.nvim.
        bo = { bufhidden = "delete" },
      },
      terminal = {
        keys = {
          term_normal = false,
        },
      },
    },
    dashboard = {
      enabled = true,
      preset = {
        keys = {
          { icon = " ", key = "f", desc = "Find File", action = ":lua Snacks.picker.files()" },
          { icon = " ", key = "n", desc = "New File", action = ":ene | startinsert" },
          { icon = " ", key = "g", desc = "Find Text", action = ":lua Snacks.picker.grep()" },
          { icon = " ", key = "r", desc = "Recent Files", action = ":lua Snacks.picker.recent()" },
          { icon = " ", key = "s", desc = "Git Status", action = ":Neogit" },
          { icon = " ", key = "o", desc = "Org Agenda", action = org.org_action("agenda.prompt") },
          { icon = " ", key = "q", desc = "Quit", action = ":qa" },
        },
        header = [[
 ███╗   ██╗ ███████╗ ██████╗  ██╗   ██╗ ██╗ ███╗   ███╗
 ████╗  ██║ ██╔════╝██╔═══██╗ ██║   ██║ ██║ ████╗ ████║
 ██╔██╗ ██║ █████╗  ██║   ██║ ██║   ██║ ██║ ██╔████╔██║
 ██║╚██╗██║ ██╔══╝  ██║   ██║ ╚██╗ ██╔╝ ██║ ██║╚██╔╝██║
 ██║ ╚████║ ███████╗╚██████╔╝  ╚████╔╝  ██║ ██║ ╚═╝ ██║
 ╚═╝  ╚═══╝ ╚══════╝ ╚═════╝    ╚═══╝   ╚═╝ ╚═╝     ╚═╝]],
      },
      sections = {
        { section = "header" },
        { section = "keys", gap = 1, padding = 1 },
        oil.dashboard_recent_files_with_oil({ limit = 5, padding = 1 }),
      },
    },
    terminal = {
      enabled = true,
      win = { style = "terminal", position = "bottom", height = 0.35 },
    },
    image = require("config.snacks_image"),
    picker = {
      enabled = true,
      main = { current = true },
      win = {
        input = {
          keys = {
            ["<Esc>"] = { "cancel", mode = { "i", "n" } },
          },
        },
      },
      sources = {
        files = {
          cmd = "rg",
          hidden = true,
          preview = preview.picker_preview,
        },
        grep = { preview = preview.picker_preview },
        grep_buffers = { preview = preview.picker_preview },
        recent = { preview = preview.picker_preview },
        projects = {
          patterns = { ".git", "package.json", "Cargo.toml", "flake.nix", "Makefile" },
        },
      },
    },
    gh = { enabled = true },
  }

  require("snacks").setup(snacks_opts)
end

return M

local oil = require("myLuaConf.oil")
local org = require("myLuaConf.org")
local preview = require("myLuaConf.plugins.snacks_preview")

local M = {}

local function snacks()
  return require("snacks")
end

---@return snacks.Config
local function opts()
  return {
    styles = {
      dashboard = {
        -- Avoid double BufDelete/BufWipeout callbacks in snacks.nvim.
        bo = { bufhidden = "delete" },
      },
      terminal = {
        stack = true,
        keys = {
          gf = function(self)
            local f = vim.fn.findfile(vim.fn.expand("<cfile>"), "**")
            if f == "" then
              Snacks.notify.warn("No file under cursor")
            else
              self:hide()
              vim.schedule(function()
                vim.cmd("e " .. f)
              end)
            end
          end,
          terminal_normal = false,
        },
      },
      input = {
        keys = {
          i_ctrl_p = { "<c-p>", { "hist_up" }, mode = { "i", "n" } },
          i_ctrl_n = { "<c-n>", { "hist_down" }, mode = { "i", "n" } },
        },
      },
    },
    dashboard = {
      enabled = true,
      preset = {
        keys = {
          { icon = " ", key = "f", desc = "Find File",    action = ":lua Snacks.picker.files()" },
          { icon = " ", key = "n", desc = "New File",     action = ":ene | startinsert" },
          { icon = " ", key = "g", desc = "Find Text",    action = ":lua Snacks.picker.grep()" },
          { icon = " ", key = "r", desc = "Recent Files", action = ":lua Snacks.picker.recent()" },
          { icon = " ", key = "s", desc = "Git Status",   action = ":Neogit" },
          { icon = " ", key = "o", desc = "Org Agenda",   action = org.action("agenda.prompt") },
          { icon = " ", key = "q", desc = "Quit",         action = ":qa" },
        },
        header = [[
 ███╗   ██╗ ███████╗ ██████╗  ██╗   ██╗ ██╗ ███╗   ███╗
 ████╗  ██║ ██╔════╝██╔═══██╗ ██║   ██║ ██║ ████╗ ████║
 ██╔██╗ ██║ █████╗  ██║   ██║ ██║   ██║ ██║ ██╔████╔██║
 ██║╚██╗██║ ██╔══╝  ██║   ██║ ╚██╗ ██╔╝ ██║ ██║╚██╔╝██║
 ██║ ╚████║ ███████╗╚██████╔╝  ╚████╔╝  ██║ ██║ ╚═╝ ██║
 ╚═╝  ╚═══╝ ╚═════╝  ╚═════╝   ╚═══╝   ╚═╝ ╚═╝     ╚═╝]],
      },
      sections = {
        { section = "header" },
        { section = "keys",  gap = 1, padding = 1 },
        oil.dashboard_recent_files_with_oil({ limit = 5, padding = 1 }),
      },
    },
    terminal = {
      enabled = true,
      auto_insert = false,
      win = {
        style = "terminal",
        position = "bottom",
        height = 0.35,
        bo = { buflisted = true },
      },
    },
    indent = {
      enabled = true,
      indent = {
        char = "│",
      },
      scope = {
        enabled = true,
      },
    },
    image = require("myLuaConf.plugins.snacks_image"),
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
end

function M.setup()
  snacks().setup(opts())
end

return M

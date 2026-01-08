local helpers = require("config.helpers")

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
          { icon = " ", key = "o", desc = "Org Agenda", action = helpers.org_action("agenda.prompt") },
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
        helpers.dashboard_recent_files_with_oil({ limit = 5, padding = 1 }),
      },
    },
    terminal = {
      enabled = true,
      win = { style = "terminal", position = "bottom", height = 0.35 },
    },
    image = require("config.snacks_image"),
    picker = {
      enabled = true,
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
          preview = helpers.bat_preview,
        },
        grep = { preview = helpers.bat_preview },
        grep_buffers = { preview = helpers.bat_preview },
        recent = { preview = helpers.bat_preview },
        projects = {
          patterns = { ".git", "package.json", "Cargo.toml", "flake.nix", "Makefile" },
        },
      },
    },
    gh = { enabled = true },
  }

  require("snacks").setup(snacks_opts)

  do
    local ok, placement = pcall(require, "snacks.image.placement")
    if ok and not placement._unhide_patch then
      placement._unhide_patch = true
      local orig_update = placement.update
      function placement:update(...)
        if self.hidden and self:ready() and #self:wins() > 0 then
          self.hidden = false
        end
        return orig_update(self, ...)
      end
    end
  end

  local dashboard = require("snacks.dashboard")
  local dashboard_cls = dashboard.Dashboard
  local orig_size = dashboard_cls.size
  local orig_update = dashboard_cls.update

  function dashboard_cls:size()
    if not self.win or not vim.api.nvim_win_is_valid(self.win) then
      return self._size or { width = 0, height = 0 }
    end
    return orig_size(self)
  end

  function dashboard_cls:update(...)
    if not self.win or not vim.api.nvim_win_is_valid(self.win) then
      return
    end
    if vim.api.nvim_win_get_buf(self.win) ~= self.buf then
      local win = vim.fn.bufwinid(self.buf)
      if win == -1 then
        return
      end
      self.win = win
    end
    return orig_update(self, ...)
  end
end

return M

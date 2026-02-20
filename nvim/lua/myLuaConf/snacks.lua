local oil = require("myLuaConf.oil")
local org = require("myLuaConf.org")
local snacks_preview = require("myLuaConf.plugins.snacks_preview")

local M = {}

local function snacks()
  return require("snacks")
end

local function reset_preview_state()
  ---@module "rs_snacks_preview"
  local ok, loaded_preview = pcall(require, "rs_snacks_preview")
  if not ok or type(loaded_preview) ~= "table" then
    return
  end
  ---@type rs_snacks_preview
  local preview = loaded_preview
  preview.reset_state()
end

local function project_confirm_winlocal(picker, item)
  local Snacks = snacks()
  picker:close()
  if not item then
    return
  end
  local dir = item.file
  if not dir or dir == "" then
    return
  end
  vim.cmd("lcd " .. vim.fn.fnameescape(dir))
  local session = Snacks.dashboard.sections.session()
  if session then
    local session_loaded = false
    vim.api.nvim_create_autocmd("SessionLoadPost", {
      once = true,
      callback = function()
        session_loaded = true
      end,
    })
    vim.defer_fn(function()
      if not session_loaded then
        Snacks.picker.files({ cwd = dir })
      end
    end, 100)
    local action = session.action
    if type(action) == "string" then
      if action:sub(1, 1) == ":" then
        vim.cmd(action:sub(2))
      else
        local keys = vim.api.nvim_replace_termcodes(action, true, true, true)
        vim.api.nvim_feedkeys(keys, "tm", true)
      end
    else
      Snacks.picker.files({ cwd = dir })
    end
  else
    Snacks.picker.files({ cwd = dir })
  end
end

---@return snacks.Config
local function opts()
  return {
    animate = {
      duration = 20,
      easing = "outQuad",
      fps = 120,
    },
    ---@type table<string, snacks.win.Config>
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
          term_normal = false,
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
    },
    scope = {
      enabled = true,
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
          preview = snacks_preview.picker_preview,
        },
        grep = { preview = snacks_preview.picker_preview },
        grep_buffers = { preview = snacks_preview.picker_preview },
        recent = { preview = snacks_preview.picker_preview },
        projects = {
          recent = true,
          patterns = { ".git", "package.json", "Cargo.toml", "flake.nix", "Makefile" },
          confirm = project_confirm_winlocal,
        },
      },
    },
    profiler = {},
    gh = { enabled = true },
  }
end

function M.setup()
  reset_preview_state()
  snacks().setup(opts())
end

return M

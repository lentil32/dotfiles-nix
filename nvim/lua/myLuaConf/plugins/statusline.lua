local function sidekick_status()
  local ok, status = pcall(require, "sidekick.status")
  if ok then
    return status
  end
  return nil
end

local project = require("myLuaConf.project")
local util = require("myLuaConf.util")

local function copilot_icon()
  return vim.fn.nr2char(0xF4B8) .. " "
end

local function project_icon()
  return vim.fn.nr2char(0xEA62) .. " "
end

local function statusline_winid()
  local winid = tonumber(util.get_var(nil, "statusline_winid"))
  if not winid or winid == 0 then
    return nil
  end
  if not vim.api.nvim_win_is_valid(winid) then
    return nil
  end
  return winid
end

local function project_root()
  local winid = statusline_winid()
  if not winid then
    return project.project_root()
  end
  local ok, root = pcall(vim.api.nvim_win_call, winid, project.project_root)
  if ok then
    return root
  end
  return nil
end

local function project_label()
  local root = project_root()
  if not root or root == "" then
    return nil
  end
  local name = vim.fn.fnamemodify(root, ":t")
  if name == "" then
    name = vim.fn.fnamemodify(root, ":~")
  end
  return project_icon() .. name
end

return {
  {
    "lualine.nvim",
    for_cat = "general",
    event = "DeferredUIEnter",
    after = function()
      local Snacks = require("snacks")
      local opts = {
        options = {
          theme = "auto",
          globalstatus = false,
        },
        sections = {
          lualine_a = { "mode" },
          lualine_b = { "branch", "diff", "diagnostics" },
          lualine_c = { "filename" },
          lualine_x = { "overseer", "fileformat", "filetype" },
          lualine_y = { "progress" },
          lualine_z = { "location" },
        },
        inactive_sections = {
          lualine_a = {},
          lualine_b = {},
          lualine_c = { "filename" },
          lualine_x = { "location" },
          lualine_y = {},
          lualine_z = {},
        },
        extensions = { "oil" },
      }

      table.insert(opts.sections.lualine_x, Snacks.profiler.status())

      table.insert(opts.sections.lualine_c, 1, {
        function()
          return project_label()
        end,
        cond = function()
          return project_root() ~= nil
        end,
        color = function()
          return "Directory"
        end,
      })

      table.insert(opts.inactive_sections.lualine_c, 1, {
        function()
          return project_label()
        end,
        cond = function()
          return project_root() ~= nil
        end,
        color = function()
          return "Directory"
        end,
      })

      table.insert(opts.sections.lualine_c, {
        function()
          return copilot_icon()
        end,
        color = function()
          local status = sidekick_status()
          if not status then
            return nil
          end
          local info = status.get()
          if info then
            return info.kind == "Error" and "DiagnosticError" or info.busy and "DiagnosticWarn" or "Special"
          end
        end,
        cond = function()
          local status = sidekick_status()
          return status and status.get() ~= nil
        end,
      })

      require("lualine").setup(opts)
    end,
  },
}

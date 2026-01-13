local function sidekick_status()
  local ok, status = pcall(require, "sidekick.status")
  if ok then
    return status
  end
  return nil
end

local function copilot_icon()
  return vim.fn.nr2char(0xF4B8) .. " "
end

local function cli_icon()
  return vim.fn.nr2char(0xEE0D) .. " "
end

return {
  {
    "lualine.nvim",
    for_cat = "general",
    event = "DeferredUIEnter",
    after = function()
      local opts = {
        options = {
          theme = "auto",
          globalstatus = false,
        },
        sections = {
          lualine_a = { "mode" },
          lualine_b = { "branch", "diff", "diagnostics" },
          lualine_c = { "filename" },
          lualine_x = { "overseer", "encoding", "fileformat", "filetype" },
          lualine_y = { "progress" },
          lualine_z = { "location" },
        },
      }

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

      table.insert(opts.sections.lualine_x, 2, {
        function()
          local status = sidekick_status()
          local sessions = status and status.cli() or {}
          return cli_icon() .. (#sessions > 1 and #sessions or "")
        end,
        cond = function()
          local status = sidekick_status()
          return status and #status.cli() > 0
        end,
        color = function()
          return "Special"
        end,
      })

      require("lualine").setup(opts)
    end,
  },
}

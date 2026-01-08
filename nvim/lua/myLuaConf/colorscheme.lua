local M = {}

function M.apply()
  if not pcall(vim.cmd.colorscheme, "modus_vivendi") then
    vim.notify("Colorscheme modus_vivendi not found", vim.log.levels.WARN)
  end
end

return M

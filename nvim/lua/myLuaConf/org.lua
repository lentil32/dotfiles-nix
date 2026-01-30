local M = {}

local configured = false

function M.setup()
  if configured then
    return
  end
  configured = true
  local home = vim.loop.os_homedir() or vim.fn.expand("~")
  require("orgmode").setup({
    org_agenda_files = { home .. "/org/**/*" },
    org_default_notes_file = home .. "/org/refile.org",
  })
end

function M.action(action)
  return function()
    M.setup()
    require("orgmode").action(action)
  end
end

return M

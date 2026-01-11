local M = {}

local configured = false

function M.setup()
  if configured then
    return
  end
  configured = true
  require("orgmode").setup({
    org_agenda_files = { "~/org/**/*" },
    org_default_notes_file = "~/org/refile.org",
  })
end

function M.action(action)
  return function()
    M.setup()
    require("orgmode").action(action)
  end
end

return M

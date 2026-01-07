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

return M

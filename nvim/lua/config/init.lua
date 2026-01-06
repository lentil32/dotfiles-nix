local M = {}

function M.setup()
  require("config.startup").setup()

  local keymaps = require("config.keymaps")
  keymaps.setup()

  require("config.autocmds").setup()
end

return M

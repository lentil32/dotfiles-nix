local M = {}

---@module "monokai-pro"
---@type MonokaiPro
local monokai = require("monokai-pro")

function M.apply()
  ---@type MonokaiPro.Config
  local opts = {
    devicons = true,
    filter = "octagon",
  }

  monokai.setup(opts)
  vim.cmd.colorscheme("monokai-pro")
end

return M

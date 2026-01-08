local keymaps = require("config.keymaps")

return {
  {
    "which-key.nvim",
    event = "DeferredUIEnter",
    after = function()
      local wk = require("which-key")
      wk.setup({ delay = 300 })
      wk.add(keymaps.list())
    end,
  },
}

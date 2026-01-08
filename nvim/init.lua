-- Leader keys (must be set before plugins)
vim.g.mapleader = " "
vim.g.maplocalleader = ","

require("nixCatsUtils").setup({
  non_nix_value = true,
})

require("myLuaConf")

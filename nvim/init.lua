-- Leader keys (must be set before plugins)
vim.g.mapleader = " "
vim.g.maplocalleader = ","

-- Python filetype plugins call has("python3") during FileType. This config does
-- not provide a pynvim host, so keep provider detection from aborting FileType.
vim.g.loaded_python3_provider = 0

vim.loader.enable()

require("myLuaConf")

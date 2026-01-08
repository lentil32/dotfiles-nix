-- Leader keys (must be set before plugins)
vim.g.mapleader = " "
vim.g.maplocalleader = ","

-- Options
vim.opt.number = true
vim.opt.relativenumber = true
vim.opt.expandtab = true
vim.opt.shiftwidth = 2
vim.opt.tabstop = 2
vim.opt.showtabline = 0
vim.opt.ignorecase = true
vim.opt.smartcase = true
vim.opt.termguicolors = true
vim.opt.clipboard = "unnamedplus"
vim.opt.undofile = true
vim.opt.signcolumn = "yes"
vim.opt.cursorline = true

-- Load plugins via lze
require("plugins")

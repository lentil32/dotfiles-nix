---@meta

---@class nvimrs_theme_switcher.ThemeSpec
---@field name string
---@field colorscheme string

---@class nvimrs_theme_switcher.OpenArgs
---@field themes nvimrs_theme_switcher.ThemeSpec[]
---@field title? string
---@field current_colorscheme? string
---@field state_path? string

---@class nvimrs_theme_switcher
---@field open fun(args: nvimrs_theme_switcher.OpenArgs)
---@field cycle_next fun(args: nvimrs_theme_switcher.OpenArgs)
---@field cycle_prev fun(args: nvimrs_theme_switcher.OpenArgs)
---@field move_next fun()
---@field move_prev fun()
---@field confirm fun()
---@field cancel fun()
---@field close fun()

---@type nvimrs_theme_switcher
local M = {}

---@param args nvimrs_theme_switcher.OpenArgs
function M.open(args) end

---@param args nvimrs_theme_switcher.OpenArgs
function M.cycle_next(args) end

---@param args nvimrs_theme_switcher.OpenArgs
function M.cycle_prev(args) end

function M.move_next() end

function M.move_prev() end

function M.confirm() end

function M.cancel() end

function M.close() end

return M

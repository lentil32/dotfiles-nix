---@meta

---@alias rs_smear_cursor.Value nil|boolean|integer|number|string|table

---@class rs_smear_cursor.SetupOpts
---@field smear_terminal_mode? boolean
---@field filetypes_disabled? string[]
---@field [string] rs_smear_cursor.Value

local M = {}

---@return integer
function M.ping() end

---@generic T: table
---@param args T
---@return T
function M.echo(args) end

---@param args table
---@return table
function M.step(args) end

---@param opts? rs_smear_cursor.SetupOpts
function M.setup(opts) end

function M.on_key() end

---@param opts? rs_smear_cursor.SetupOpts
function M.toggle(opts) end

return M

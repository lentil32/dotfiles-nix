---@meta

local M = {}

function M.setup() end

function M.switch_to_last_buffer() end

---@param win? integer
---@return integer|nil
function M.oil_last_buf_for_win(win) end

return M

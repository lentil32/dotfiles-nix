---@meta

---@alias rs_plugin_util.OptionValue nil|boolean|integer|number|string|table

local M = {}

---@param path? string
---@return boolean
function M.is_dir(path) end

---@param buf integer|nil
---@param opts table<string, rs_plugin_util.OptionValue>
function M.set_buf_opts(buf, opts) end

---@param win integer|nil
---@param opts table<string, rs_plugin_util.OptionValue>
function M.set_win_opts(win, opts) end

---@overload fun(buf: integer|nil, opt: string): rs_plugin_util.OptionValue
---@generic T
---@param buf integer|nil
---@param opt string
---@param default T
---@return T
function M.get_buf_opt(buf, opt, default) end

---@overload fun(win: integer|nil, opt: string): rs_plugin_util.OptionValue
---@generic T
---@param win integer|nil
---@param opt string
---@param default T
---@return T
function M.get_win_opt(win, opt, default) end

---@overload fun(buf: integer|nil, name: string): rs_plugin_util.OptionValue
---@generic T
---@param buf integer|nil
---@param name string
---@param default T
---@return T
function M.get_var(buf, name, default) end

---@param path? string
function M.edit_path(path) end

function M.patch_oil_parse_url() end

---@param path? string
function M.open_oil(path) end

---@return string
function M.oil_winbar() end

function M.oil_select_other_window() end

function M.goto_definition_other_window() end

function M.delete_current_buffer() end

function M.kill_window_and_buffer() end

---@return integer|nil
function M.other_window() end

---@return integer win
---@return boolean created
function M.get_or_create_other_window() end

return M

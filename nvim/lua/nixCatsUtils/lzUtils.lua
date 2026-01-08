local util = require("myLuaConf.util")
local M = {}

local function get_cat_enabled(cat, default)
  if util.get_var(nil, "nixCats-special-rtp-entry-nixCats") ~= nil then
    if type(_G.nixCats) == "function" then
      return _G.nixCats(cat) or false
    end
    local ok, nc = pcall(require, "nixCats")
    if ok and type(nc.get) == "function" then
      return nc.get(cat) or false
    end
    return false
  end
  return default
end

-- NixCats-specific lze handler for category-gated specs.
-- Register before calling `lze.load`:
-- require("lze").register_handlers(require("nixCatsUtils.lzUtils").for_cat)
M.for_cat = {
  spec_field = "for_cat",
  set_lazy = false,
  modify = function(plugin)
    if type(plugin.for_cat) == "table" and plugin.for_cat.cat ~= nil then
      plugin.enabled = get_cat_enabled(plugin.for_cat.cat, plugin.for_cat.default)
    else
      plugin.enabled = get_cat_enabled(plugin.for_cat, false)
    end
    return plugin
  end,
}

return M

local rust = require("snacks_preview")
local util = require("myLuaConf.util")

local M = {}

function M.picker_preview(ctx)
  local Snacks = require("snacks")
  local ret = Snacks.picker.preview.file(ctx)
  local path = Snacks.picker.util.path(ctx.item)
  if not path or util.is_dir(path) then
    rust.close_doc_preview(ctx.buf)
    return ret
  end
  rust.attach_doc_preview({ buf = ctx.buf, win = ctx.win, path = path })
  return ret
end

return M

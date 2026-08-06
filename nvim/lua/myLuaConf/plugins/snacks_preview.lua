local M = {}

function M.picker_preview(ctx)
  local Snacks = require("snacks")
  ---@module "nvimrs_snacks_preview"
  local preview = require("nvimrs_snacks_preview")
  preview.close_doc_preview_for_window(ctx.win)

  local ret = Snacks.picker.preview.file(ctx)
  local path = Snacks.picker.util.path(ctx.item)

  if not path or vim.fn.isdirectory(path) == 1 then
    return ret
  end

  preview.attach_doc_preview({ buf = ctx.buf, win = ctx.win, path = path })
  return ret
end

return M

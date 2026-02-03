local M = {}

function M.picker_preview(ctx)
  local Snacks = require("snacks")
  return Snacks.picker.preview.file(ctx)
end

return M

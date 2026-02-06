local M = {}

function M.try_require(mod)
  local ok, ret = pcall(require, mod)
  if ok then
    return ret
  end
  return nil
end

return M

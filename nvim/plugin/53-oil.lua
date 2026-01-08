local ok = pcall(require, "oil")
if not ok then
  return
end

require("config.oil").setup()

local ok = pcall(require, "snacks")
if not ok then
  return
end

require("config.snacks").setup()

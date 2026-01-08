if not pcall(require, "snacks") then
  return
end

require("config.autocmds").setup()

require("lze").load({
  {
    "nvim-lint",
    for_cat = "lint",
    event = "FileType",
    after = function()
      local lint = require("lint")
      local js_ts = { "biomejs" }
      lint.linters_by_ft = {
        javascript = js_ts,
        javascriptreact = js_ts,
        typescript = js_ts,
        typescriptreact = js_ts,
        lua = { "selene" },
        rust = { "clippy" },
      }

      local group = vim.api.nvim_create_augroup("UserLint", { clear = true })
      vim.api.nvim_create_autocmd({ "BufWritePost" }, {
        group = group,
        callback = function()
          lint.try_lint()
        end,
      })
    end,
  },
})

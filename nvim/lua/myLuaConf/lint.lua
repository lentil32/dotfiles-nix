require("lze").load({
  {
    "nvim-lint",
    for_cat = "lint",
    event = "FileType",
    after = function()
      local lint = require("lint")
      lint.linters_by_ft = {
        javascript = { "biomejs" },
        javascriptreact = { "biomejs" },
        typescript = { "biomejs" },
        typescriptreact = { "biomejs" },
        jsonc = { "biomejs" },
        lua = { "selene" },
        rust = { "clippy" },
      }

      vim.api.nvim_create_autocmd({ "BufWritePost" }, {
        callback = function()
          lint.try_lint()
        end,
      })
    end,
  },
})

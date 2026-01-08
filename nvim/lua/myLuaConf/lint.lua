require("lze").load({
  {
    "nvim-lint",
    for_cat = "lint",
    event = "FileType",
    after = function()
      require("lint").linters_by_ft = {
        -- configure linters here
      }

      local group = vim.api.nvim_create_augroup("UserLint", { clear = true })
      vim.api.nvim_create_autocmd({ "BufWritePost" }, {
        group = group,
        callback = function()
          require("lint").try_lint()
        end,
      })
    end,
  },
})

require("lze").load({
  {
    "conform.nvim",
    for_cat = "format",
    event = { "BufReadPre", "BufNewFile" },
    after = function()
      require("conform").setup({
        default_format_opts = {
          lsp_format = "prefer",
        },
        formatters_by_ft = {
          rust = { "rustfmt", lsp_format = "never" },
          ["yaml.ansible"] = { "yamlfmt", lsp_format = "never" },
        },
        format_on_save = {
          timeout_ms = 500,
        },
      })
    end,
  },
})

require("lze").load({
  {
    "conform.nvim",
    for_cat = "format",
    event = { "BufReadPre", "BufNewFile" },
    after = function()
      require("conform").setup({
        formatters_by_ft = {
          javascript = { "biome" },
          typescript = { "biome" },
          javascriptreact = { "biome" },
          typescriptreact = { "biome" },
          json = { "biome" },
          jsonc = { "biome" },
          lua = { "stylua" },
          rust = { "rustfmt" },
          toml = { "taplo" },
          yaml = { "yamlfmt" },
          ["yaml.ansible"] = { "yamlfmt" },
        },
        format_on_save = {
          timeout_ms = 500,
          lsp_fallback = true,
        },
      })
    end,
  },
})

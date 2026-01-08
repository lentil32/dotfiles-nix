return {
  {
    "nvim-lspconfig",
    for_cat = "lsp",
    event = "BufReadPre",
    after = function()
      local capabilities = require("blink.cmp").get_lsp_capabilities()

      vim.lsp.config("nil_ls", { capabilities = capabilities })
      vim.lsp.config("rust_analyzer", { capabilities = capabilities })
      vim.lsp.config("lua_ls", {
        capabilities = capabilities,
        settings = { Lua = { diagnostics = { globals = { "vim" } } } },
      })
      vim.lsp.config("ruff", { capabilities = capabilities })
      vim.lsp.config("biome", { capabilities = capabilities })

      vim.lsp.enable({ "nil_ls", "rust_analyzer", "lua_ls", "ruff", "biome" })
    end,
  },
}

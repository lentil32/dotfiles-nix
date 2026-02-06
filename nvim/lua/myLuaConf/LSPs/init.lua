local catUtils = require("nixCatsUtils")
local lua_helpers = require("myLuaConf.lua_helpers")

local capabilities
do
  local blink = lua_helpers.try_require("blink.cmp")
  if blink and blink.get_lsp_capabilities then
    capabilities = blink.get_lsp_capabilities()
  end
end
local on_attach = require("myLuaConf.LSPs.on_attach")

require("lze").load({
  {
    "nvim-lspconfig",
    for_cat = "lsp",
    on_require = { "lspconfig" },
    lsp = function(plugin)
      vim.lsp.config(plugin.name, plugin.lsp or {})
      vim.lsp.enable(plugin.name)
    end,
    before = function(_)
      local base = { on_attach = on_attach }
      if capabilities then
        base.capabilities = capabilities
      end
      vim.lsp.config("*", base)
    end,
  },
  {
    "mason.nvim",
    enabled = not catUtils.isNixCats,
    on_plugin = { "nvim-lspconfig" },
    load = function(name)
      vim.cmd.packadd(name)
      vim.cmd.packadd("mason-lspconfig.nvim")
      require("mason").setup()
      require("mason-lspconfig").setup({ automatic_installation = true })
    end,
  },
  {
    "lazydev.nvim",
    for_cat = "lsp",
    cmd = { "LazyDev" },
    ft = "lua",
    after = function()
      require("lazydev").setup({
        library = {
          { words = { "nixCats" }, path = (nixCats.nixCatsPath or "") .. "/lua" },
        },
      })
    end,
  },
  {
    "lua_ls",
    enabled = nixCats("lsp") or false,
    lsp = {
      filetypes = { "lua" },
      settings = {
        Lua = {
          diagnostics = { globals = { "vim" } },
          workspace = {
            library = vim.api.nvim_get_runtime_file("", true),
            checkThirdParty = false,
          },
        },
      },
    },
  },
  {
    "nil_ls",
    enabled = nixCats("lsp") or false,
    lsp = {
      filetypes = { "nix" },
    },
  },
  {
    "rust_analyzer",
    enabled = nixCats("lsp") or false,
    lsp = {
      filetypes = { "rust" },
      settings = {
        ["rust-analyzer"] = {
          procMacro = {
            ignored = {
              leptos_macro = { "server" },
            },
          },
        },
      },
    },
  },
  {
    "ruff",
    enabled = nixCats("lsp") or false,
    lsp = {
      filetypes = { "python" },
    },
  },
  {
    "biome",
    enabled = nixCats("lsp") or false,
    lsp = {
      filetypes = { "javascript", "javascriptreact", "typescript", "typescriptreact", "json" },
    },
  },
  {
    "jsonls",
    enabled = nixCats("lsp") or false,
    lsp = {
      filetypes = { "json", "jsonc" },
    },
  },
  {
    "taplo",
    enabled = nixCats("lsp") or false,
    lsp = {
      filetypes = { "toml" },
    },
  },
  {
    "yamlls",
    enabled = nixCats("lsp") or false,
    lsp = {
      filetypes = { "yaml" },
    },
  },
  {
    "vtsls",
    enabled = nixCats("lsp") or false,
    lsp = {
      filetypes = {
        "javascript",
        "javascriptreact",
        "javascript.jsx",
        "typescript",
        "typescriptreact",
        "typescript.tsx",
      },
    },
  },
})

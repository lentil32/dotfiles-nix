local catUtils = require("nixCatsUtils")
local on_attach = require("myLuaConf.LSPs.on_attach")

---@module "blink.cmp"
local blink = require("blink.cmp")
local capabilities = blink.get_lsp_capabilities()

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
      vim.lsp.config("*", { on_attach = on_attach, capabilities = capabilities })
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
    on_require = { "lazydev" },
    after = function()
      require("lazydev").setup({
        library = {
          { path = "monokai-pro.nvim", mods = { "monokai-pro" } },
          { words = { "nixCats" }, path = nixCats.nixCatsPath .. "/lua" },
          {
            path = nixCats.nixCatsPath .. "/lua/myLuaConf/types",
            mods = {
              "rs_project_root",
              "rs_plugin_util",
              "rs_readline",
              "rs_text",
              "rs_autocmds",
              "rs_snacks_preview",
              "rs_smear_cursor",
              "rs_theme_switcher",
            },
          },
        },
      })
    end,
  },
  {
    "emmylua_ls",
    enabled = nixCats("lsp") or false,
    lsp = {
      filetypes = { "lua" },
      settings = {
        Lua = {
          diagnostics = {
            globals = { "vim" },
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

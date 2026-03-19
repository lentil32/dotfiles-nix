local catUtils = require("nixCatsUtils")
local on_attach = require("myLuaConf.LSPs.on_attach")

---@module "blink.cmp"
local blink = require("blink.cmp")
local capabilities = blink.get_lsp_capabilities()

local oxfmt_filetypes = {
  "javascript",
  "javascriptreact",
  "javascript.jsx",
  "typescript",
  "typescriptreact",
  "typescript.tsx",
  "toml",
  "json",
  "jsonc",
  "json5",
  "yaml",
  "html",
  "vue",
  "handlebars",
  "hbs",
  "css",
  "scss",
  "less",
  "graphql",
  "markdown",
  "mdx",
}

local oxlint_filetypes = {
  "javascript",
  "javascriptreact",
  "javascript.jsx",
  "typescript",
  "typescriptreact",
  "typescript.tsx",
  "vue",
  "svelte",
  "astro",
}

local function oxlint_on_attach(client, bufnr)
  on_attach(client, bufnr)

  vim.api.nvim_buf_create_user_command(bufnr, "LspOxlintFixAll", function()
    client:request_sync("workspace/executeCommand", {
      command = "oxc.fixAll",
      arguments = {
        {
          uri = vim.uri_from_bufnr(bufnr),
        },
      },
    }, nil, bufnr)
  end, { desc = "Apply all fixable oxlint diagnostics" })
end

local function on_attach_without_formatting(client, bufnr)
  on_attach(client, bufnr)
  client.server_capabilities.documentFormattingProvider = false
  client.server_capabilities.documentRangeFormattingProvider = false
end

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
    "oxfmt",
    enabled = nixCats("lsp") or false,
    lsp = {
      cmd = { "oxfmt", "--lsp" },
      filetypes = oxfmt_filetypes,
      -- Oxfmt works without a dedicated config file, so keep project-level fallbacks.
      root_markers = {
        { ".oxfmtrc.json", ".oxfmtrc.jsonc" },
        { "package.json" },
        { ".git" },
      },
      workspace_required = true,
    },
  },
  {
    "oxlint",
    enabled = nixCats("lsp") or false,
    lsp = {
      before_init = function(_, config)
        config.settings = config.settings or {}
      end,
      cmd = { "oxlint", "--lsp" },
      filetypes = oxlint_filetypes,
      on_attach = oxlint_on_attach,
      root_markers = { ".oxlintrc.json", "oxlint.config.ts" },
      settings = {},
      workspace_required = true,
    },
  },
  {
    "jsonls",
    enabled = nixCats("lsp") or false,
    lsp = {
      filetypes = { "json", "jsonc" },
      on_attach = on_attach_without_formatting,
    },
  },
  {
    "taplo",
    enabled = nixCats("lsp") or false,
    lsp = {
      filetypes = { "toml" },
      on_attach = on_attach_without_formatting,
    },
  },
  {
    "yamlls",
    enabled = nixCats("lsp") or false,
    lsp = {
      filetypes = { "yaml" },
      on_attach = on_attach_without_formatting,
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
      on_attach = on_attach_without_formatting,
    },
  },
})

local lze = require("lze")

-- Theme (load immediately)
vim.cmd.colorscheme("modus_vivendi")

-- Blink.cmp (startup plugin - needed for LSP capabilities)
require("blink.cmp").setup({
  keymap = {
    preset = "default",
    ["<C-space>"] = { "show", "show_documentation", "hide_documentation" },
    ["<C-e>"] = { "hide" },
    ["<CR>"] = { "accept", "fallback" },
    ["<Tab>"] = { "select_next", "snippet_forward", "fallback" },
    ["<S-Tab>"] = { "select_prev", "snippet_backward", "fallback" },
    ["<C-b>"] = { "scroll_documentation_up", "fallback" },
    ["<C-f>"] = { "scroll_documentation_down", "fallback" },
  },
  sources = {
    default = { "lsp", "path", "buffer" },
  },
  completion = {
    documentation = { auto_show = true },
    trigger = {
      show_on_insert_on_trigger_character = true,
    },
    list = {
      selection = { preselect = true, auto_insert = false },
    },
    menu = {
      auto_show = true,
    },
  },
})

-- Snacks.nvim (startup plugin)
require("snacks").setup({
  dashboard = {
    enabled = true,
    preset = {
      keys = {
        { icon = " ", key = "f", desc = "Find File", action = ":Telescope find_files" },
        { icon = " ", key = "n", desc = "New File", action = ":ene | startinsert" },
        { icon = " ", key = "g", desc = "Find Text", action = ":Telescope live_grep" },
        { icon = " ", key = "r", desc = "Recent Files", action = ":Telescope oldfiles" },
        { icon = " ", key = "s", desc = "Git Status", action = ":Neogit" },
        { icon = " ", key = "o", desc = "Org Agenda", action = ":lua require('orgmode').action('agenda.prompt')" },
        { icon = " ", key = "q", desc = "Quit", action = ":qa" },
      },
      header = [[
 ███╗   ██╗ ███████╗ ██████╗  ██╗   ██╗ ██╗ ███╗   ███╗
 ████╗  ██║ ██╔════╝██╔═══██╗ ██║   ██║ ██║ ████╗ ████║
 ██╔██╗ ██║ █████╗  ██║   ██║ ██║   ██║ ██║ ██╔████╔██║
 ██║╚██╗██║ ██╔══╝  ██║   ██║ ╚██╗ ██╔╝ ██║ ██║╚██╔╝██║
 ██║ ╚████║ ███████╗╚██████╔╝  ╚████╔╝  ██║ ██║ ╚═╝ ██║
 ╚═╝  ╚═══╝ ╚══════╝ ╚═════╝    ╚═══╝   ╚═╝ ╚═╝     ╚═╝]],
    },
  },
  terminal = {
    enabled = true,
    win = {
      style = "terminal",
      position = "float",
      border = "rounded",
    },
  },
})

-- Terminal keymaps
vim.keymap.set("t", "<Esc><Esc>", [[<C-\><C-n>]], { desc = "Exit terminal mode" })
vim.keymap.set("t", "<C-\\>", function() Snacks.terminal.toggle() end, { desc = "Toggle terminal" })
vim.keymap.set("n", "<C-\\>", function() Snacks.terminal.toggle() end, { desc = "Toggle terminal" })

-- Load plugins with lze
lze.load({
  -- Which-key (load on leader key)
  {
    "which-key.nvim",
    event = "DeferredUIEnter",
    after = function()
      local wk = require("which-key")
      wk.setup({ delay = 300 })
      wk.add({
        { "<leader>/", "<cmd>Telescope live_grep<cr>", desc = "Search project" },

        { "<leader>f", group = "file" },
        { "<leader>ff", "<cmd>Yazi<cr>", desc = "Browse files" },
        { "<leader>fr", "<cmd>Telescope oldfiles<cr>", desc = "Recent files" },
        { "<leader>fs", "<cmd>w<cr>", desc = "Save" },

        { "<leader>fy", group = "yank" },
        { "<leader>fyy", function() vim.fn.setreg("+", vim.fn.expand("%:t")) end, desc = "Filename" },
        { "<leader>fyY", function() vim.fn.setreg("+", vim.fn.expand("%:p")) end, desc = "Full path" },
        { "<leader>fyd", function() vim.fn.setreg("+", vim.fn.expand("%:p:h")) end, desc = "Directory" },
        { "<leader>fyr", function() vim.fn.setreg("+", vim.fn.expand("%")) end, desc = "Relative path" },

        { "<leader>p", group = "project" },
        { "<leader>pf", "<cmd>Telescope find_files<cr>", desc = "Find file" },

        { "<leader>b", group = "buffer" },
        { "<leader>bb", "<cmd>Telescope buffers<cr>", desc = "Buffers" },
        { "<leader>bd", "<cmd>bdelete<cr>", desc = "Delete" },

        { "<leader>s", group = "search" },
        { "<leader>sp", "<cmd>Telescope live_grep<cr>", desc = "Project" },
        { "<leader>ss", "<cmd>Telescope current_buffer_fuzzy_find<cr>", desc = "Buffer" },

        { "<leader>g", group = "git" },
        { "<leader>gs", "<cmd>Neogit<cr>", desc = "Status" },

        { "<leader>o", group = "org" },
        { "<leader>oa", "<cmd>lua require('orgmode').action('agenda.prompt')<cr>", desc = "Agenda" },
        { "<leader>oc", "<cmd>lua require('orgmode').action('capture.prompt')<cr>", desc = "Capture" },

        { "<leader>l", group = "lsp" },
        { "<leader>la", vim.lsp.buf.code_action, desc = "Action" },
        { "<leader>ld", vim.lsp.buf.definition, desc = "Definition" },
        { "<leader>lf", "<cmd>lua require('conform').format()<cr>", desc = "Format" },
        { "<leader>lr", vim.lsp.buf.rename, desc = "Rename" },
        { "<leader>lh", vim.lsp.buf.hover, desc = "Hover" },
        { "<leader>li", "<cmd>TSToolsOrganizeImports<cr>", desc = "Organize imports" },
        { "<leader>lu", "<cmd>TSToolsRemoveUnused<cr>", desc = "Remove unused" },
        { "<leader>lm", "<cmd>TSToolsAddMissingImports<cr>", desc = "Add missing imports" },

        { "<leader>'", function() Snacks.terminal.toggle() end, desc = "Terminal" },

        { "<leader>w", group = "window" },
        { "<leader>wh", "<C-w>h", desc = "Left" },
        { "<leader>wj", "<C-w>j", desc = "Down" },
        { "<leader>wk", "<C-w>k", desc = "Up" },
        { "<leader>wl", "<C-w>l", desc = "Right" },
        { "<leader>wv", "<cmd>vsplit<cr>", desc = "Vsplit" },
        { "<leader>ws", "<cmd>split<cr>", desc = "Split" },
        { "<leader>wd", "<cmd>close<cr>", desc = "Close" },
      })
    end,
  },

  -- Telescope (load on command)
  {
    "telescope.nvim",
    cmd = "Telescope",
    after = function()
      require("telescope").setup({})
      require("telescope").load_extension("fzf")
    end,
  },

  -- Yazi (load on command)
  {
    "yazi.nvim",
    cmd = "Yazi",
    after = function()
      require("yazi").setup({ open_for_directories = true })
    end,
  },

  -- Neogit (load on command)
  {
    "neogit",
    cmd = "Neogit",
    after = function()
      require("neogit").setup({
        integrations = { diffview = true, telescope = true },
      })
    end,
  },

  -- Gitsigns (load on file open)
  {
    "gitsigns.nvim",
    event = "BufReadPre",
    after = function()
      require("gitsigns").setup({})
    end,
  },

  -- Grug-far (search and replace)
  {
    "grug-far.nvim",
    cmd = "GrugFar",
    after = function()
      require("grug-far").setup({})
    end,
  },

  -- Orgmode (load on .org files)
  {
    "orgmode",
    ft = "org",
    after = function()
      require("orgmode").setup({
        org_agenda_files = { "~/org/**/*" },
        org_default_notes_file = "~/org/refile.org",
      })
    end,
  },

  -- Treesitter (load early for syntax)
  {
    "nvim-treesitter",
    event = "BufReadPre",
    after = function()
      require("nvim-treesitter.configs").setup({
        highlight = { enable = true },
        indent = { enable = true },
      })
    end,
  },

  -- LSP (load on file open)
  {
    "nvim-lspconfig",
    event = "BufReadPre",
    after = function()
      local lsp = require("lspconfig")
      local capabilities = require("blink.cmp").get_lsp_capabilities()

      lsp.nil_ls.setup({ capabilities = capabilities })
      lsp.rust_analyzer.setup({ capabilities = capabilities })
      lsp.lua_ls.setup({
        capabilities = capabilities,
        settings = { Lua = { diagnostics = { globals = { "vim" } } } },
      })
      lsp.ruff.setup({ capabilities = capabilities })
      lsp.biome.setup({ capabilities = capabilities })
    end,
  },

  -- TypeScript tools (load on TS files)
  {
    "typescript-tools.nvim",
    ft = { "typescript", "typescriptreact", "javascript", "javascriptreact" },
    after = function()
      local capabilities = require("blink.cmp").get_lsp_capabilities()
      require("typescript-tools").setup({
        capabilities = capabilities,
        settings = {
          tsserver_file_preferences = {
            includeInlayParameterNameHints = "all",
            includeInlayFunctionParameterTypeHints = true,
            includeInlayVariableTypeHints = true,
          },
        },
      })
    end,
  },

  -- Conform (formatting)
  {
    "conform.nvim",
    event = "BufWritePre",
    after = function()
      require("conform").setup({
        formatters_by_ft = {
          javascript = { "biome" },
          typescript = { "biome" },
          javascriptreact = { "biome" },
          typescriptreact = { "biome" },
          json = { "biome" },
        },
        format_on_save = {
          timeout_ms = 500,
          lsp_fallback = true,
        },
      })
    end,
  },

})

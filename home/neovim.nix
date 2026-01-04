{ pkgs, ... }:
{
  programs.yazi = {
    enable = true;
    enableZshIntegration = true;
    plugins = {
      starship = pkgs.fetchFromGitHub {
        owner = "Rolv-Apneseth";
        repo = "starship.yazi";
        rev = "eca186171c5f2011ce62712f95f699308251c749";
        hash = "sha256-xcz2+zepICZ3ji0Hm0SSUBSaEpabWUrIdG7JmxUl/ts=";
      };
    };
    initLua = ''
      require("starship"):setup()
    '';
  };

  programs.neovim = {
    enable = true;
    vimAlias = true;
    defaultEditor = true;

    extraPackages = with pkgs; [
      ripgrep
      fd
      biome # TS/JS formatter & linter
    ];

    plugins = with pkgs.vimPlugins; [
      # Theme (same as spacemacs)
      {
        plugin = modus-themes-nvim;
        type = "lua";
        config = ''
          require("modus-themes").setup({})
          vim.cmd.colorscheme("modus_vivendi")
        '';
      }

      # Core
      lazy-nvim
      plenary-nvim

      # Fuzzy finder (like consult/vertico)
      telescope-fzf-native-nvim
      {
        plugin = telescope-nvim;
        type = "lua";
        config = ''
          require("telescope").setup({})
          require("telescope").load_extension("fzf")
        '';
      }

      # Which-key (SPC menu like spacemacs)
      {
        plugin = which-key-nvim;
        type = "lua";
        config = ''require("which-key").setup({ delay = 300 })'';
      }

      # File manager
      {
        plugin = yazi-nvim;
        type = "lua";
        config = ''require("yazi").setup({ open_for_directories = true })'';
      }

      # Git (like magit)
      diffview-nvim
      {
        plugin = neogit;
        type = "lua";
        config = ''
          require("neogit").setup({
            integrations = { diffview = true, telescope = true },
          })
        '';
      }
      {
        plugin = gitsigns-nvim;
        type = "lua";
        config = ''require("gitsigns").setup({})'';
      }

      # Org mode
      {
        plugin = orgmode;
        type = "lua";
        config = ''
          require("orgmode").setup({
            org_agenda_files = { "~/org/**/*" },
            org_default_notes_file = "~/org/refile.org",
          })
        '';
      }

      # Treesitter
      {
        plugin = nvim-treesitter.withAllGrammars;
        type = "lua";
        config = ''
          require("nvim-treesitter.configs").setup({
            highlight = { enable = true },
            indent = { enable = true },
          })
        '';
      }

      # LSP (like eglot)
      {
        plugin = nvim-lspconfig;
        type = "lua";
        config = ''
          local lsp = require("lspconfig")
          lsp.nil_ls.setup({})        -- Nix
          lsp.rust_analyzer.setup({}) -- Rust
          lsp.lua_ls.setup({ settings = { Lua = { diagnostics = { globals = { "vim" } } } } })
          lsp.ruff.setup({})          -- Python
          lsp.biome.setup({})         -- TS/JS linting
        '';
      }

      # TypeScript (better than ts_ls)
      {
        plugin = typescript-tools-nvim;
        type = "lua";
        config = ''
          require("typescript-tools").setup({
            settings = {
              tsserver_file_preferences = {
                includeInlayParameterNameHints = "all",
                includeInlayFunctionParameterTypeHints = true,
                includeInlayVariableTypeHints = true,
              },
            },
          })
        '';
      }

      # Formatting
      {
        plugin = conform-nvim;
        type = "lua";
        config = ''
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
        '';
      }

      # Completion
      cmp-nvim-lsp
      cmp-buffer
      cmp-path
      {
        plugin = nvim-cmp;
        type = "lua";
        config = ''
          local cmp = require("cmp")
          cmp.setup({
            mapping = cmp.mapping.preset.insert({
              ["<C-Space>"] = cmp.mapping.complete(),
              ["<CR>"] = cmp.mapping.confirm({ select = true }),
              ["<Tab>"] = cmp.mapping.select_next_item(),
              ["<S-Tab>"] = cmp.mapping.select_prev_item(),
            }),
            sources = cmp.config.sources(
              { { name = "nvim_lsp" }, { name = "orgmode" } },
              { { name = "buffer" }, { name = "path" } }
            ),
          })
        '';
      }
    ];

    extraLuaConfig = ''
      vim.g.mapleader = " "
      vim.g.maplocalleader = ","

      vim.opt.number = true
      vim.opt.relativenumber = true
      vim.opt.expandtab = true
      vim.opt.shiftwidth = 2
      vim.opt.ignorecase = true
      vim.opt.smartcase = true
      vim.opt.termguicolors = true
      vim.opt.clipboard = "unnamedplus"
      vim.opt.undofile = true

      -- Spacemacs-style keybindings
      local wk = require("which-key")
      wk.add({
        { "<leader>/", "<cmd>Telescope live_grep<cr>", desc = "Search project" },

        { "<leader>f", group = "file" },
        { "<leader>ff", "<cmd>Yazi<cr>", desc = "Browse files" },
        { "<leader>fr", "<cmd>Telescope oldfiles<cr>", desc = "Recent files" },
        { "<leader>fs", "<cmd>w<cr>", desc = "Save" },

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

        { "<leader>w", group = "window" },
        { "<leader>wv", "<cmd>vsplit<cr>", desc = "Vsplit" },
        { "<leader>ws", "<cmd>split<cr>", desc = "Split" },
        { "<leader>wd", "<cmd>close<cr>", desc = "Close" },
      })
    '';
  };
}

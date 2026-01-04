{
  pkgs,
  inputs,
  ...
}:
let
  nixCats = inputs.nixCats;
in
{
  imports = [ nixCats.homeModule ];

  # Yazi file manager
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
      smart-enter = pkgs.fetchFromGitHub {
        owner = "yazi-rs";
        repo = "plugins";
        rev = "03cdd4b5b15341b3c0d0f4c850d633fadd05a45f";
        hash = "sha256-5dMAJ6W/L66XuH4CCwRRFpKSLy0ZDFIABAYleFX0AsQ=";
      } + "/smart-enter.yazi";
      vcs-files = pkgs.fetchFromGitHub {
        owner = "yazi-rs";
        repo = "plugins";
        rev = "03cdd4b5b15341b3c0d0f4c850d633fadd05a45f";
        hash = "sha256-5dMAJ6W/L66XuH4CCwRRFpKSLy0ZDFIABAYleFX0AsQ=";
      } + "/vcs-files.yazi";
    };
    initLua = ''
      require("starship"):setup()
      require("zoxide"):setup({ update_db = true })
    '';
    # Use new 'mgr' instead of deprecated 'manager'
    # See: https://github.com/sxyazi/yazi/pull/2803
    keymap = {
      mgr.prepend_keymap = [
        {
          on = "l";
          run = "plugin smart-enter";
          desc = "Enter the child directory, or open the file";
        }
        {
          on = [ "g" "s" ];
          run = ''shell 'nvim -c "Neogit"' --block'';
          desc = "Open Neogit";
        }
        {
          on = [ "g" "f" ];
          run = "plugin vcs-files";
          desc = "Show Git file changes";
        }
      ];
    };
  };

  # Neovide (GUI frontend for neovim)
  programs.neovide = {
    enable = true;
  };

  # nixCats neovim configuration
  nixCats = {
    enable = true;
    packageNames = [ "nvim" ];
    luaPath = "${../nvim}";

    categoryDefinitions.replace =
      {
        pkgs,
        settings,
        categories,
        extra,
        name,
        mkPlugin,
        ...
      }@packageDef:
      {
        # Plugins that load at startup
        startupPlugins = {
          general = with pkgs.vimPlugins; [
            modus-themes-nvim
            plenary-nvim
            lze
            telescope-fzf-native-nvim
            snacks-nvim
          ];

          completion = with pkgs.vimPlugins; [
            blink-cmp
          ];
        };

        # Plugins loaded via lze (packadd)
        optionalPlugins = {
          general = with pkgs.vimPlugins; [
            which-key-nvim
            telescope-nvim
            yazi-nvim
            diffview-nvim
          ];

          git = with pkgs.vimPlugins; [
            neogit
            gitsigns-nvim
            grug-far-nvim
          ];

          treesitter = with pkgs.vimPlugins; [
            nvim-treesitter.withAllGrammars
          ];

          lsp = with pkgs.vimPlugins; [
            nvim-lspconfig
            conform-nvim
          ];

          typescript = with pkgs.vimPlugins; [
            typescript-tools-nvim
          ];

          org = with pkgs.vimPlugins; [
            orgmode
          ];
        };

        # External packages (LSPs, formatters, etc.)
        lspsAndRuntimeDeps = {
          general = with pkgs; [
            ripgrep
            fd
          ];

          lsp = with pkgs; [
            nil
            rust-analyzer
            lua-language-server
            ruff
          ];

          typescript = with pkgs; [
            biome
            nodePackages.typescript-language-server
          ];
        };
      };

    packageDefinitions.replace = {
      nvim =
        { pkgs, name, ... }:
        {
          settings = {
            aliases = [
              "vim"
              "vi"
            ];
            wrapRc = true;
          };

          categories = {
            general = true;
            git = true;
            lsp = true;
            treesitter = true;
            completion = true;
            typescript = true;
            org = true;
          };
        };
    };
  };
}

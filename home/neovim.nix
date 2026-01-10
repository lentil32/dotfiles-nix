{
  pkgs,
  pkgs-unstable,
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
    package = pkgs-unstable.yazi;
    enableZshIntegration = true;
    plugins = {
      starship = pkgs.fetchFromGitHub {
        owner = "Rolv-Apneseth";
        repo = "starship.yazi";
        rev = "eca186171c5f2011ce62712f95f699308251c749";
        hash = "sha256-xcz2+zepICZ3ji0Hm0SSUBSaEpabWUrIdG7JmxUl/ts=";
      };
      smart-enter =
        pkgs.fetchFromGitHub {
          owner = "yazi-rs";
          repo = "plugins";
          rev = "03cdd4b5b15341b3c0d0f4c850d633fadd05a45f";
          hash = "sha256-5dMAJ6W/L66XuH4CCwRRFpKSLy0ZDFIABAYleFX0AsQ=";
        }
        + "/smart-enter.yazi";
      vcs-files =
        pkgs.fetchFromGitHub {
          owner = "yazi-rs";
          repo = "plugins";
          rev = "03cdd4b5b15341b3c0d0f4c850d633fadd05a45f";
          hash = "sha256-5dMAJ6W/L66XuH4CCwRRFpKSLy0ZDFIABAYleFX0AsQ=";
        }
        + "/vcs-files.yazi";
      projects = pkgs.fetchFromGitHub {
        owner = "MasouShizuka";
        repo = "projects.yazi";
        rev = "eed0657a833f56ea69f3531c89ecc7bad761d611";
        hash = "sha256-5J0eqffUzI0GodpqwzmaQJtfh75kEbbIwbR8pFH/ZmU=";
      };
      fr = pkgs.fetchFromGitHub {
        owner = "lpnh";
        repo = "fr.yazi";
        rev = "aa88cd4d4345c07345275291c1a236343f834c86";
        hash = "sha256-3D1mIQpEDik0ppPQo+/NIhCxEu/XEnJMJ0HiAFxlOE4=";
      };
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
          on = [
            "g"
            "s"
          ];
          run = ''shell 'nvim -c "Neogit"' --block'';
          desc = "Open Neogit";
        }
        {
          on = [
            "g"
            "f"
          ];
          run = "plugin vcs-files";
          desc = "Show Git file changes";
        }
        {
          on = [
            "g"
            "p"
          ];
          run = "plugin projects";
          desc = "Switch project";
        }
        {
          on = [
            "g"
            "P"
          ];
          run = "plugin projects --args=save";
          desc = "Save as project";
        }
        {
          on = [
            "f"
            "r"
          ];
          run = "plugin fr rg";
          desc = "Search file by content (rg)";
        }
        {
          on = [
            "f"
            "a"
          ];
          run = "plugin fr rga";
          desc = "Search file by content (rga)";
        }
      ];
    };
  };

  # Neovide (GUI frontend for neovim)
  programs.neovide = {
    enable = true;
    settings = {
      font = {
        normal = [ "Iosevka Nerd Font" ];
        size = 14.0;
      };
    };
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
            lzextras
            snacks-nvim
            grug-far-nvim # search/replace
            project-nvim # projectile-like project management
            oil-nvim
          ];

          completion = with pkgs.vimPlugins; [
            blink-cmp
          ];
        };

        # Plugins loaded via lze (packadd)
        optionalPlugins = {
          general = with pkgs.vimPlugins; [
            which-key-nvim
            flash-nvim
            hop-nvim
            nvim-surround
            smear-cursor-nvim
            sidekick-nvim
          ];

          git = with pkgs.vimPlugins; [
            neogit
            diffview-nvim
            gitsigns-nvim
            git-blame-nvim
            vim-flog
          ];

          treesitter = with pkgs.vimPlugins; [
            nvim-treesitter.withAllGrammars
          ];

          lsp = with pkgs.vimPlugins; [
            nvim-lspconfig
            mason-nvim
            mason-lspconfig-nvim
            lazydev-nvim
          ];

          format = with pkgs.vimPlugins; [
            conform-nvim
          ];

          lint = with pkgs.vimPlugins; [
            nvim-lint
          ];

          typescript = with pkgs.vimPlugins; [
            nvim-vtsls
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
              bat
              imagemagick
              mermaid-cli
              typst
              tectonic
            ];

          lsp = with pkgs; [
            nil
            rust-analyzer
            lua-language-server
            ruff
          ];

          typescript = with pkgs; [
            biome
            vtsls
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
            format = true;
            lint = true;
            treesitter = true;
            completion = true;
            typescript = true;
            org = true;
          };
        };
    };
  };
}

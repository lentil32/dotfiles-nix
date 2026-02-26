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
      let
        lib = pkgs.lib;
        vim = pkgs.vimPlugins;
        p = pkgs;
        rustWorkspace = ../nvim/rust;
        rustLockHashes = import ../nvim/rust/lock-hashes.nix;
        rustCargoLock = {
          lockFile = rustWorkspace + "/Cargo.lock";
          outputHashes = rustLockHashes.byCrate;
        };
        toKebab = lib.strings.replaceStrings [ "_" ] [ "-" ];
        mkPname = crate: "${toKebab crate}-nvim";
        mkInstallPhase = libBase: outBase: ''
          runHook preInstall
          mkdir -p $out/lua
          lib=""
          if [ -f target/release/lib${libBase}.dylib ]; then
            lib=target/release/lib${libBase}.dylib
          elif [ -f target/release/lib${libBase}.so ]; then
            lib=target/release/lib${libBase}.so
          elif [ -f target/release/${libBase}.dll ]; then
            lib=target/release/${libBase}.dll
          else
            lib=$(find target -type f \( -name "lib${libBase}.dylib" -o -name "lib${libBase}.so" -o -name "${libBase}.dll" \) | head -n 1)
          fi
          if [ -z "$lib" ]; then
            echo "${libBase} library not found" >&2
            exit 1
          fi
          case "$lib" in
            *.dll) cp "$lib" "$out/lua/${outBase}.dll" ;;
            *.dylib|*.so) cp "$lib" "$out/lua/${outBase}.so" ;;
            *)
              echo "${libBase} library not found: $lib" >&2
              exit 1
              ;;
          esac
          runHook postInstall
        '';
        mkRustPlugin =
          {
            crate,
            pname ? mkPname crate,
            libBase ? crate,
            outBase ? libBase,
            cargoBuildFlags ? [
              "--locked"
              "--package"
              crate
            ],
          }:
          pkgs.rustPlatform.buildRustPackage {
            inherit pname;
            version = "0.1.0";
            src = rustWorkspace;
            cargoLock = rustCargoLock;
            cargoBuildFlags = cargoBuildFlags;
            doCheck = false;
            installPhase = mkInstallPhase libBase outBase;
          };
        rustPluginOrder = [
          "rs_project_root"
          "rs_plugin_util"
          "rs_readline"
          "rs_text"
          "rs_snacks_preview"
          "rs_autocmds"
          "rs_smear_cursor"
          "rs_theme_switcher"
        ];
        rustPluginList = map (crate: mkRustPlugin { inherit crate; }) rustPluginOrder;
        categoriesConfig = {
          general = {
            startupPlugins = [
              vim.monokai-pro-nvim
              vim.kanagawa-nvim
              vim.modus-themes-nvim
              vim.nvim-web-devicons
              vim.plenary-nvim
              vim.lze
              vim.lzextras
              vim.snacks-nvim
              vim.grug-far-nvim # search/replace
              vim.oil-nvim
            ]
            ++ rustPluginList;
            optionalPlugins = [
              vim.which-key-nvim
              vim.flash-nvim
              vim.hop-nvim
              vim.nvim-autopairs
              vim.nvim-surround
              vim.sidekick-nvim
              vim.overseer-nvim
              (mkPlugin "witch-line" (
                pkgs.fetchFromGitHub {
                  owner = "sontungexpt";
                  repo = "witch-line";
                  rev = "929a5e9f7ff05bf412507a79c285955ad9e54c3f";
                  hash = "sha256-QK4rIm/DiBFGlZo2/hRgMhDi8W5MU9DYqq0AAJqGMiI=";
                }
              ))
            ];
            runtimeDeps = [
              p.ripgrep
              p.fd
              p.bat
              p.imagemagick
              p.mermaid-cli
              p.typst
              p.tectonic
            ];
          };

          completion = {
            startupPlugins = [
              vim.blink-cmp
            ];
          };

          git = {
            optionalPlugins = [
              vim.neogit
              vim.diffview-nvim
              vim.gitsigns-nvim
              vim.git-blame-nvim
              vim.vim-flog
            ];
          };

          treesitter = {
            optionalPlugins = [
              vim.nvim-treesitter.withAllGrammars
            ];
          };

          lsp = {
            optionalPlugins = [
              vim.nvim-lspconfig
              vim.mason-nvim
              vim.mason-lspconfig-nvim
              vim.lazydev-nvim
            ];
            runtimeDeps = [
              p.nil
              p.rust-analyzer
              p.emmylua-ls
              p.ruff
              p.yaml-language-server
            ];
          };

          format = {
            optionalPlugins = [
              vim.conform-nvim
            ];
            runtimeDeps = [
              p.taplo
              p.yamlfmt
            ];
          };

          lint = {
            optionalPlugins = [
              vim.nvim-lint
            ];
            runtimeDeps = [
              p.yamllint
            ];
          };

          typescript = {
            optionalPlugins = [
              vim.nvim-vtsls
            ];
            runtimeDeps = [
              p.biome
              p.vtsls
            ];
          };

          org = {
            optionalPlugins = [
              vim.orgmode
            ];
          };
        };
        collect =
          field:
          lib.filterAttrs (_: value: value != [ ]) (
            lib.mapAttrs (_: cfg: cfg.${field} or [ ]) categoriesConfig
          );
        startupPlugins = collect "startupPlugins";
        optionalPlugins = collect "optionalPlugins";
        lspsAndRuntimeDeps = collect "runtimeDeps";
      in
      {
        inherit startupPlugins optionalPlugins lspsAndRuntimeDeps;
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
            suffix-path = false;
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

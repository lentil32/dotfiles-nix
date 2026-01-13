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
        rustWorkspace = ../nvim/rust;
        projectRootPlugin = pkgs.rustPlatform.buildRustPackage {
          pname = "project-root-nvim";
          version = "0.1.0";
          src = rustWorkspace;
          cargoLock = {
            lockFile = rustWorkspace + "/Cargo.lock";
          };
          cargoBuildFlags = [
            "--locked"
            "--package"
            "project_root"
          ];
          doCheck = false;
          installPhase = ''
            runHook preInstall
            mkdir -p $out/lua
            lib=""
            if [ -f target/release/libproject_root.dylib ]; then
              lib=target/release/libproject_root.dylib
            elif [ -f target/release/libproject_root.so ]; then
              lib=target/release/libproject_root.so
            elif [ -f target/release/project_root.dll ]; then
              lib=target/release/project_root.dll
            else
              lib=$(find target -type f \( -name "libproject_root.dylib" -o -name "libproject_root.so" -o -name "project_root.dll" \) | head -n 1)
            fi
            if [ -z "$lib" ]; then
              echo "project_root library not found" >&2
              exit 1
            fi
            case "$lib" in
              *.dll) cp "$lib" "$out/lua/project_root.dll" ;;
              *.dylib|*.so) cp "$lib" "$out/lua/project_root.so" ;;
              *)
                echo "project_root library not found: $lib" >&2
                exit 1
                ;;
            esac
            runHook postInstall
          '';
        };
        utilPlugin = pkgs.rustPlatform.buildRustPackage {
          pname = "my-util-nvim";
          version = "0.1.0";
          src = rustWorkspace;
          cargoLock = {
            lockFile = rustWorkspace + "/Cargo.lock";
          };
          cargoBuildFlags = [
            "--locked"
            "--package"
            "my_util"
          ];
          doCheck = false;
          installPhase = ''
            runHook preInstall
            mkdir -p $out/lua
            lib=""
            if [ -f target/release/libmy_util.dylib ]; then
              lib=target/release/libmy_util.dylib
            elif [ -f target/release/libmy_util.so ]; then
              lib=target/release/libmy_util.so
            elif [ -f target/release/my_util.dll ]; then
              lib=target/release/my_util.dll
            else
              lib=$(find target -type f \( -name "libmy_util.dylib" -o -name "libmy_util.so" -o -name "my_util.dll" \) | head -n 1)
            fi
            if [ -z "$lib" ]; then
              echo "my_util library not found" >&2
              exit 1
            fi
            case "$lib" in
              *.dll) cp "$lib" "$out/lua/my_util.dll" ;;
              *.dylib|*.so) cp "$lib" "$out/lua/my_util.so" ;;
              *)
                echo "my_util library not found: $lib" >&2
                exit 1
                ;;
            esac
            runHook postInstall
          '';
        };
        autocmdsPlugin = pkgs.rustPlatform.buildRustPackage {
          pname = "my-autocmds-nvim";
          version = "0.1.0";
          src = rustWorkspace;
          cargoLock = {
            lockFile = rustWorkspace + "/Cargo.lock";
          };
          cargoBuildFlags = [
            "--locked"
            "--package"
            "my_autocmds"
          ];
          doCheck = false;
          installPhase = ''
            runHook preInstall
            mkdir -p $out/lua
            lib=""
            if [ -f target/release/libmy_autocmds.dylib ]; then
              lib=target/release/libmy_autocmds.dylib
            elif [ -f target/release/libmy_autocmds.so ]; then
              lib=target/release/libmy_autocmds.so
            elif [ -f target/release/my_autocmds.dll ]; then
              lib=target/release/my_autocmds.dll
            else
              lib=$(find target -type f \( -name "libmy_autocmds.dylib" -o -name "libmy_autocmds.so" -o -name "my_autocmds.dll" \) | head -n 1)
            fi
            if [ -z "$lib" ]; then
              echo "my_autocmds library not found" >&2
              exit 1
            fi
            case "$lib" in
              *.dll) cp "$lib" "$out/lua/my_autocmds.dll" ;;
              *.dylib|*.so) cp "$lib" "$out/lua/my_autocmds.so" ;;
              *)
                echo "my_autocmds library not found: $lib" >&2
                exit 1
                ;;
            esac
            runHook postInstall
          '';
        };
      in
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
            oil-nvim
            autocmdsPlugin
            utilPlugin
            projectRootPlugin
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
            nvim-autopairs
            nvim-surround
            smear-cursor-nvim
            sidekick-nvim
            overseer-nvim
            lualine-nvim
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

          format = with pkgs; [
            stylua
          ];

          lint = with pkgs; [
            selene
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

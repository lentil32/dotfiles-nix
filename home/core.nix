{
  pkgs,
  pkgs-unstable,
  system,
  ...
}:
{
  home.packages = with pkgs; [
    # archives
    zip
    xz
    unzip
    p7zip

    # utils
    awscli2
    bottom
    convmv
    fasd
    fd
    ffmpeg
    fzf
    glow # markdown previewer in terminal
    gh
    keycastr # keystroke visualizer
    lefthook
    nurl
    # Signing git commits in macOS
    # Set up a GPG key for signing Git commits on MacOS (M1)
    # Reference: https://gist.github.com/phortuin/cf24b1cca3258720c71ad42977e1ba57
    pinentry_mac
    pngpaste
    jq
    yarn
    yq-go # yaml processer https://github.com/mikefarah/yq
    wabt
    websocat
    wireguard-tools

    aria2 # A lightweight multi-protocol & multi-source command-line download utility
    socat # replacement of openbsd-netcat
    nmap # A utility for network discovery and security auditing

    # programming
    (aider-chat.withOptional {
      withPlaywright = true;
      withBedrock = true;
    })
    bats # Bash automated testing
    cmake
    corepack_latest # Node.js dependency bridge
    llvm
    opam
    mysql84
    nodejs_24
    turbo
    typescript
    typescript-language-server

    # Rust overlay
    (rust-bin.stable.latest.default.override {
      extensions = [
        "rust-analyzer"
        "clippy"
      ];
    })

    # LSPs, formatters, linters
    clang-tools
    emacs-lsp-booster
    ispell
    lua-language-server
    multimarkdown
    nixfmt-rfc-style
    ruff
    shfmt
    taplo
    vscode-langservers-extracted
    yaml-language-server
    yapf

    # Productivity

    # misc
    caddy
    coreutils
    cowsay
    exercism
    file
    which
    tree
    gnupg
    gnused
    gnutar
    gawk
    pandoc
    postgresql
    typst
    typstfmt
    zstd
  ];

  programs = {
    bat = {
      enable = true;
      config.theme = "Monokai Extended";
      extraPackages = with pkgs.bat-extras; [
        batdiff
        batpipe
        batgrep
        batwatch
      ];
    };
    bun = {
      enable = true;
      package = pkgs-unstable.bun;
      settings.telemetry = false;
    };
    direnv = {
      enable = true;
      enableZshIntegration = true;
      mise.enable = true;
      nix-direnv.enable = true;
    };
    eza = {
      enable = true;
      git = true;
      enableZshIntegration = true;
    };
    java.enable = true;
    mise = {
      enable = true;
      enableZshIntegration = true;
      globalConfig = {
        alias = {
          nix = "aqua:https://github.com/joshbode/mise-nix";
        };
        settings = {
          idiomatic_version_file_enable_tools = [ "bun" ];
        };
        # Not works yet... due to hook non-existence
        # tools = {
        #   "nix" = "latest";
        # };
      };
    };
    texlive = {
      enable = true;
      extraPackages = tpkgs: { inherit (tpkgs) collection-latexextra dvipng; };
    };
    ripgrep = {
      enable = true;
      arguments = [
        "--max-columns=150"
        "--max-columns-preview"
        "--smart-case"
      ];
    };
    zoxide.enable = true;
  };
}

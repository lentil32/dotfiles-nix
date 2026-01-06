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
    csvkit # Claude Code loves it
    fasd
    fd
    ffmpeg
    fzf
    glow # markdown previewer in terminal
    gh
    keycastr # keystroke visualizer
    killport
    lefthook
    nurl
    # Signing git commits in macOS
    # Set up a GPG key for signing Git commits on MacOS (M1)
    # Reference: https://gist.github.com/phortuin/cf24b1cca3258720c71ad42977e1ba57
    pass
    pinentry_mac
    pngpaste
    jq
    imagemagick
    yq-go # yaml processer https://github.com/mikefarah/yq
    wabt
    websocat
    wireguard-tools

    aria2 # A lightweight multi-protocol & multi-source command-line download utility
    ripgrep-all # rga - search in PDFs, Office docs, archives, etc.
    socat # replacement of openbsd-netcat
    nmap # A utility for network discovery and security auditing

    # programming
    # (pkgs-unstable.aider-chat.withOptional {
    #   withPlaywright = true;
    #   withBedrock = true;
    # })
    bats # Bash automated testing
    cmake
    corepack_latest # Node.js dependency bridge
    llvm
    mysql84
    nodejs_24
    mermaid-cli
    opam
    turbo
    typescript

    # Rust overlay
    (rust-bin.stable.latest.default.override {
      targets = [ "wasm32-unknown-unknown" ];
      extensions = [
        "rust-analyzer"
        "clippy"
      ];
    })
    wasm-pack
    wasm-bindgen-cli

    # LSPs, formatters, linters
    clang-tools
    emacs-lsp-booster
    eslint # maybe needed by flycheck
    ispell
    lua-language-server
    multimarkdown
    nil
    nixfmt-rfc-style
    pkgs-unstable.typescript-go
    ruff
    shfmt
    taplo
    typescript-language-server
    vscode-langservers-extracted
    yaml-language-server
    yapf

    # Productivity

    # LLM

    # misc
    caddy
    coreutils
    cowsay
    exercism
    file
    ghostscript
    which
    tree
    gnupg
    gnused
    gnutar
    gawk
    pandoc
    postgresql
    typst
    typstyle
    zstd
  ];

  programs = {
    atuin = {
      enable = true;
      flags = [ "--disable-up-arrow" ];
    };
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
          experimental = true;
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

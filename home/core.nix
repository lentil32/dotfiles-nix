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

    # AI
    pkgs-unstable.aider-chat

    # utils
    awscli2
    bottom
    colima # docker runtime with minimal setup
    convmv
    docker
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
    python3Packages.playwright # web scraper. `aider` uses it
    ripgrep
    jq
    yarn
    yq-go # yaml processer https://github.com/mikefarah/yq
    websocat
    wireguard-tools

    aria2 # A lightweight multi-protocol & multi-source command-line download utility
    socat # replacement of openbsd-netcat
    nmap # A utility for network discovery and security auditing

    # programming
    bats # Bash automated testing
    cmake
    corepack_latest # Node.js dependency bridge
    llvm
    opam
    mysql84
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
    eslint
    hadolint # docker linter
    ispell
    multimarkdown
    nixfmt-rfc-style
    ruff
    shfmt
    taplo
    vscode-langservers-extracted
    yapf

    # fonts
    iosevka-comfy.comfy
    source-sans-pro

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
      nix-direnv.enable = true;
    };
    eza = {
      enable = true;
      git = true;
      enableZshIntegration = true;
    };
    java.enable = true;
    texlive = {
      enable = true;
      extraPackages = tpkgs: { inherit (tpkgs) collection-latexextra dvipng; };
    };
    zoxide.enable = true;
  };
}

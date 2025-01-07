{ pkgs, ... }:
{
  home.packages = with pkgs; [
    # archives
    zip
    xz
    unzip
    p7zip

    # utils
    bottom
    fasd
    fd
    ffmpeg
    fzf
    glow # markdown previewer in terminal
    nurl
    # Signing git commits in macOS
    # Set up a GPG key for signing Git commits on MacOS (M1)
    # Reference: https://gist.github.com/phortuin/cf24b1cca3258720c71ad42977e1ba57
    pinentry_mac
    pngpaste
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
    nodePackages_latest.ts-node
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
    eslint
    ispell
    multimarkdown
    nixfmt-rfc-style
    nodePackages_latest.prettier
    ruff
    shfmt
    taplo
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
      extraPackages = tpkgs: { inherit (tpkgs) collection-latexextra; };
    };
    zoxide.enable = true;
  };
}

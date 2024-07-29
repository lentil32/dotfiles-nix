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
    fzf
    glow # markdown previewer in terminal
    nurl
    pngpaste
    ripgrep
    jq
    yt-dlp
    yq-go # yaml processer https://github.com/mikefarah/yq
    wireguard-tools

    aria2 # A lightweight multi-protocol & multi-source command-line download utility
    socat # replacement of openbsd-netcat
    nmap # A utility for network discovery and security auditing

    # programming
    cmake
    llvm
    opam

    # python
    pipenv
    poetry
    pyenv

    # rust
    rustc
    cargo
    clippy
    rustfmt

    # LSPs and formatters
    black
    clang-tools
    ispell
    multimarkdown
    nixfmt-rfc-style
    pyright
    ruff
    taplo

    # fonts
    iosevka-comfy.comfy

    # misc
    caddy
    cowsay
    exercism
    file
    which
    tree
    gnused
    gnutar
    gawk
    pandoc
    zstd

    nodePackages.js-beautify
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
    direnv = {
      enable = true;
      nix-direnv.enable = true;
    };
    eza.enable = true;
    thefuck.enable = true;
    zoxide.enable = true;
  };
}

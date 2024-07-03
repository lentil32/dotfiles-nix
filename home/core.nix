{ pkgs, ... }:
{
  home.packages = with pkgs; [
    # archives
    zip
    xz
    unzip
    p7zip

    # utils
    fasd
    fd
    fzf
    glow # markdown previewer in terminal
    ripgrep
    jq
    bottom
    yt-dlp
    yq-go # yaml processer https://github.com/mikefarah/yq
    wireguard-tools

    aria2 # A lightweight multi-protocol & multi-source command-line download utility
    socat # replacement of openbsd-netcat
    nmap # A utility for network discovery and security auditing

    # programming
    cmake
    devenv
    llvm
    opam
    freetype
    pkg-config # matplotlib requires it

    # python
    pipenv
    poetry
    pyenv

    # LSPs and formatters
    black
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
    file
    which
    tree
    gnused
    gnutar
    gawk
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

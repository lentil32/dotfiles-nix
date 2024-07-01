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
    llvm
    opam
    pyenv

    # LSPs and formatters
    ispell
    nixfmt-rfc-style
    ruff
    taplo

    # fonts
    iosevka-comfy.comfy

    # misc
    cowsay
    file
    which
    tree
    gnused
    gnutar
    gawk
    zstd
    caddy
    gnupg

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
    thefuck = {
      enable = true;
    };

    zoxide = {
      enable = true;
    };
  };
}

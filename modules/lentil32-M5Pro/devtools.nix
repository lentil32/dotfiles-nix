{ pkgs, ... }:
let
  # Full Xcode is expected on the host for Apple-provided toolchains such as
  # metal/metallib; keep the usual Unix build essentials explicit here.
  buildEssentials = with pkgs; [
    gnumake
    pkg-config
    cmake
    ninja
    libiconv
    autoconf
    automake
    libtool
    m4
  ];
in
{
  environment.systemPackages =
    with pkgs;
    [
      zsh
      git
      vim
      man-pages
      man-pages-posix
    ]
    ++ buildEssentials;

  environment.variables = {
    EDITOR = "vim";
  };
}

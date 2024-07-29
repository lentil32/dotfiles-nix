{ pkgs, ... }:
{
  home.packages = with pkgs; [
    gocode-gomod
    gopls
    gopkgs
    gotools
  ];
  programs.go = {
    enable = true;
  };
}

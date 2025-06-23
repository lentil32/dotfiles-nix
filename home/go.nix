{ pkgs, ... }:
{
  home.packages = with pkgs; [
    godef
    gogetdoc
    gomodifytags
    gopkgs
    # gopls commented out 2025-06-19: buggy with gotools
    gotests
    # gotools
    impl
    reftools
  ];
  programs.go = {
    enable = true;
    goPath = ".go";
    goBin = ".go/bin";
    packages = { };
  };
}

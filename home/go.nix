{ pkgs, ... }:
{
  home.packages = with pkgs; [
    godef
    gogetdoc
    gomodifytags
    gopkgs
    gopls
    gotests
    gotools
    impl
    reftools
  ];
  programs.go = {
    enable = true;
    goPath = ".go";
    goBin = ".go/bin";
    packages = {

    };
  };
}

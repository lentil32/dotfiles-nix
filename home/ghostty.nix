# As of 1.1.13, buggy
{ pkgs-unstable, ... }:
{
  programs.ghostty = {
    enable = true;
    package = pkgs-unstable.ghostty;
    installBatSyntax = true;
    installVimSyntax = true;
  };
}

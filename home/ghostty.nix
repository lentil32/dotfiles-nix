{ pkgs-unstable, ... }:
{
  programs.ghostty = {
    enable = true;
    package = pkgs-unstable.ghostty;
    installBatSyntax = true;
    installVimSyntax = true;
  };
}

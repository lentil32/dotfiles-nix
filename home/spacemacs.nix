{ pkgs, lib, ... }:
{
  home.packages = [
    pkgs.emacs-30
    pkgs.git
  ];

  home.activation.updateSpacemacs = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    if [ ! -d ~/.emacs.d ]; then
      ${pkgs.git}/bin/git clone https://github.com/syl20bnr/spacemacs ~/.emacs.d
      cd ~/.emacs.d
      ${pkgs.git}/bin/git checkout develop
    else
      cd ~/.emacs.d
      ${pkgs.git}/bin/git checkout develop
      ${pkgs.git}/bin/git pull
    fi
  '';
}

{
  pkgs,
  lib,
  config,
  ...
}:
let
  emacsDir = "${config.home.homeDirectory}/.emacs.d";
in
{
  home.packages = [
    pkgs.emacs-30
  ];

  home.activation.updateSpacemacs = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    emacs_dir="${emacsDir}"
    if [ ! -d "$emacs_dir" ]; then
      ${pkgs.git}/bin/git clone https://github.com/syl20bnr/spacemacs "$emacs_dir"
      cd "$emacs_dir"
      ${pkgs.git}/bin/git checkout develop
    else
      cd "$emacs_dir"
      ${pkgs.git}/bin/git checkout develop
      ${pkgs.git}/bin/git pull || echo "Failed to update Spacemacs; manual intervention may be required."
    fi
  '';
}

{ pkgs, config, ... }:
{
  home.packages = [ pkgs.emacs-30 ];
#  home.file.".emacs.d" = {
#    source = pkgs.fetchFromGitHub {
#      owner = "syl20bnr";
#      repo = "spacemacs";
#      rev = "124ffa9fda4094e81c02b3334ac2214d624a0807";
#      hash = "sha256-bnLtyyMaRRYWl0q600eeIC48tBxSXZAuIseLsln3EoU=";
#    };
#    recursive = true;
#  };
}

{ pkgs, config, ... }:
{
  home.packages = [ pkgs.emacs-29 ];
  home.file.".emacs.d" = {
    source = pkgs.fetchFromGitHub {
      owner = "syl20bnr";
      repo = "spacemacs";
      rev = "9ec2da8d3d7ea9603f5f9a7580168db8440a90ed";
      hash = "sha256-kUiu+tbBjzs/u8LPhCSVlF4uCWIoupTYcqhqq9bz8kY=";
    };
    recursive = true;
  };
}

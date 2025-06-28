{ lib, ... }:
{
  programs.starship = {
    enable = true;

    enableBashIntegration = true;
    enableZshIntegration = true;

    settings = {
      format = lib.concatStrings [
        "$all"
      ];
      character = {
        success_symbol = "[λ](bold green)";
        error_symbol = "[›](bold red)";
      };
      aws = {
        disabled = true;
        symbol = "🅰 ";
      };
      time = {
        disabled = false;
        utc_time_offset = "local";
      };
    };
  };
}

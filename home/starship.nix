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
        symbol = "🅰 ";
      };
      gcloud = {
        # do not show the account/project's info
        # to avoid the leak of sensitive information when sharing the terminal
        format = "on [$symbol$active(($region))]($style) ";
        symbol = "🅶 ️";
      };
      time = {
        disabled = false;
        utc_time_offset = "local";
      };
    };
  };
}

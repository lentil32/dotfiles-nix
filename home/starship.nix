{ lib, ... }:
{
  programs.starship = {
    enable = true;

    enableZshIntegration = true;

    settings = {
      format = lib.concatStrings [
        "$username"
        "$hostname"
        "$localip"
        "$shlvl"
        "$directory"
        "$git_branch"
        "$git_commit"
        "$git_state"
        "$git_metrics"
        "$docker_context"
        "$package"
        "$python"
        "$nix_shell"
        "$memory_usage"
        "$direnv"
        "$env_var"
        "$mise"
        "$custom"
        "$sudo"
        "$cmd_duration"
        "$line_break"
        "$jobs"
        "$battery"
        "$time"
        "$status"
        "$os"
        "$container"
        "$netns"
        "$shell"
        "$character"
      ];
      character = {
        success_symbol = "[λ](bold green)";
        error_symbol = "[›](bold red)";
      };
      aws = {
        disabled = true;
        symbol = "🅰 ";
      };
      git_status = {
        disabled = true;
      };
      time = {
        disabled = false;
        utc_time_offset = "local";
      };
    };
  };
}

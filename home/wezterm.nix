{ ... }:
{
  programs.wezterm = {
    enable = true;
    enableZshIntegration = true;
  };

  xdg.configFile."wezterm/wezterm.lua".source = ./config/wezterm.lua;
}

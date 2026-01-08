{ ... }:
{
  programs.wezterm = {
    enable = true;
    enableZshIntegration = true;
    extraConfig = ''
      local wezterm = require 'wezterm'
      local config = {}

      config.color_scheme = 'Tokyo Night'
      config.font = wezterm.font 'Iosevka'
      config.font_size = 14.0
      config.keys = {
        { key = 'Enter', mods = 'SHIFT', action = wezterm.action { SendString = '\x1b\r' } },
      }

      return config
    '';
  };
}

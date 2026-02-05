{ ... }:
{
  programs.wezterm = {
    enable = true;
    enableZshIntegration = true;
    extraConfig = ''
      local wezterm = require 'wezterm'
      local config = {}

      config.color_scheme = 'Tokyo Night'
      config.use_fancy_tab_bar = true
      config.window_frame = {
        font = wezterm.font { family = 'Iosevka Comfy', weight = 'Bold' },
        font_size = 12.0,
        active_titlebar_bg = '#1a1b26',
        inactive_titlebar_bg = '#16161e',
        active_titlebar_fg = '#c0caf5',
        inactive_titlebar_fg = '#545c7e',
        active_titlebar_border_bottom = '#1a1b26',
        inactive_titlebar_border_bottom = '#16161e',
        button_bg = '#1a1b26',
        button_fg = '#c0caf5',
        button_hover_bg = '#292e42',
        button_hover_fg = '#c0caf5',
      }
      config.colors = {
        tab_bar = {
          inactive_tab_edge = '#1f2335',
          background = '#16161e',
          active_tab = {
            bg_color = '#1a1b26',
            fg_color = '#7aa2f7',
            intensity = 'Bold',
            underline = 'None',
            italic = false,
            strikethrough = false,
          },
          inactive_tab = {
            bg_color = '#16161e',
            fg_color = '#545c7e',
          },
          inactive_tab_hover = {
            bg_color = '#292e42',
            fg_color = '#7aa2f7',
            italic = true,
          },
          new_tab = {
            bg_color = '#16161e',
            fg_color = '#7aa2f7',
          },
          new_tab_hover = {
            bg_color = '#292e42',
            fg_color = '#7aa2f7',
            italic = true,
          },
        },
      }
      config.font = wezterm.font 'Iosevka Comfy'
      config.font_size = 14.0
      config.front_end = 'WebGpu'
      config.webgpu_power_preference = 'HighPerformance'
      config.max_fps = 120
      config.animation_fps = 120
      config.scrollback_lines = 32768
      config.audible_bell = 'SystemBeep'
      config.visual_bell = {
        fade_in_duration_ms = 100,
        fade_out_duration_ms = 200,
        target = 'BackgroundColor',
      }
      config.keys = {
        { key = 'Enter', mods = 'SHIFT', action = wezterm.action { SendString = '\x1b\r' } },
      }

      return config
    '';
  };
}

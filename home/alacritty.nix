{ ... }:
{
  programs.alacritty = {
    enable = true;
    settings = {
      env.TERM = "xterm-256color";
      bell = {
        animation = "EaseOutExpo";
        color = "#ffffff";
        duration = 0;
      };
      colors = {
        draw_bold_text_with_bright_colors = false;
        # Solarized
        # primary = {
        #    background= "0x002b36";
        #    foreground= "0x839496";
        # };
      };
      cursor.unfocused_hollow = true;
      font = {
        size = 14.0;
        normal = {
          family = "Iosevka Comfy";
          style = "SemiLight";
        };
        bold = {
          family = "Iosevka Comfy";
          style = "ExtraBold";
        };
        bold_italic = {
          family = "Iosevka Comfy";
          style = "Extrabold Italic";
        };
        italic = {
          family = "Iosevka Comfy";
          style = "SemiLight Italic";
        };
        glyph_offset = {
          x = 0;
          y = 0;
        };
      };
      scrolling = {
        history = 10000;
        multiplier = 3;
      };
      window = {
        decorations = "buttonless";
        dynamic_padding = false;
        dynamic_title = true;
        opacity = 1.0;
        option_as_alt = "Both";
        startup_mode = "Windowed";
      };
      window.dimensions = {
        columns = 120;
        lines = 42;
      };
      window.padding = {
        x = 0;
        y = 0;
      };
    };
  };
}

{ pkgs, ... }:
{
  programs.tmux = {
    enable = true;

    # Set prefix key to C-a
    prefix = "C-a";

    # Set vi mode keys
    keyMode = "vi";

    # Status line configuration
    extraConfig = ''
      # Status line - left
      set -g @prefix_highlight_show_copy_mode 'on'
      set-option -g status-left "#{prefix_highlight}[#S] "

      # Status line - right
      set-option -g status-right-length 60
      set -g @batt_remain_short true
      set-option -g status-right "<#{USER}@#H> %a %Y-%m-%d %H:%M#{?#{!=:#{battery_percentage},},#{battery_status_bg} #{?#{==:#{battery_remain},},charging ,#{battery_remain} }[#{battery_percentage}],}"
    '';

    # Plugins
    plugins = with pkgs.tmuxPlugins; [
      battery
      open
      pain-control
      prefix-highlight
      sensible
      yank
    ];
  };
}

{ pkgs, ... }:
{
  programs.ghostty = {
    enable = true;
    package = pkgs.ghostty-bin;
    enableZshIntegration = true;
    installVimSyntax = true;
    settings = {
      theme = "TokyoNight";
      "font-family" = "Cascadia Code";
      "custom-shader" = "shaders/cursor_warp.glsl";
      "custom-shader-animation" = true;
      "window-save-state" = "always";
      "shell-integration-features" = "cursor,no-sudo,title,ssh-env,ssh-terminfo,path";
      "scrollback-limit" = 50000000;
      "mouse-hide-while-typing" = true;
      "window-padding-x" = 6;
      "window-padding-y" = 6;
      "background-opacity" = 0.92;
      "background-opacity-cells" = true;
      "background-blur" = 20;
      keybind = "shift+enter=text:\\n";
      "macos-option-as-alt" = true;
    };
  };

  xdg.configFile."ghostty/shaders" = {
    source = ./config/ghostty/shaders;
    recursive = true;
  };
}

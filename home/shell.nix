{
  pkgs,
  config,
  lib,
  ...
}:
{
  programs.bash.enable = true;
  # Options: https://mynixos.com/home-manager/options/programs.zsh
  programs.zsh = {
    enable = true;
    dotDir = "${config.xdg.configHome}/zsh";
    prezto = {
      enable = true;
      pmodules = [
        # List of modules: https://github.com/sorin-ionescu/prezto/tree/master/modules#readme
        "environment"
        "terminal"
        "editor"
        "history"
        "directory"
        "spectrum"
        "utility"
        "completion"
        "prompt"
        "autosuggestions"
        "fasd"
      ];
    };
    sessionVariables = {
    };
    envExtra = ''
      setopt aliases
      alias python=python3
    '';
    initContent = ''
      # Yazi wrapper - cd to directory on exit
      function y() {
        local tmp="$(mktemp -t "yazi-cwd.XXXXXX")" cwd
        command yazi "$@" --cwd-file="$tmp"
        IFS= read -r -d "" cwd < "$tmp"
        [ -n "$cwd" ] && [ "$cwd" != "$PWD" ] && builtin cd -- "$cwd"
        rm -f -- "$tmp"
      }

      # Ensure Emacs keybindings in ZLE
      bindkey -e
    '';

    shellGlobalAliases = {
      "--help" = "--help 2>&1 | bat --language=help --style=plain";
    };
    shellAliases = rec {
      ".." = "cd ..";

      # Reference: https://github.com/starcraft66/os-config/blob/master/home-manager/programs/zsh.nix
      # Alias eza for ls command: https://gist.github.com/AppleBoiy/04a249b6f64fd0fe1744aff759a0563b
      ls = "eza";
      l = "eza -lbF --git";
      ll = "eza -lbGF --git";
      llm = "eza -lbGd --git --sort=modified";
      la = "eza -lbhHigUmuSa --time-style=long-iso --git --color-scale";
      lx = "eza -lbhHigUmuSa@ --time-style=long-iso --git --color-scale";

      # specialty views
      lS = "eza -1";
      lt = "eza --tree --level=2";
      "l." = "eza -a | grep -E '^.'";
      tree = "${ls} --tree";
      cdtemp = "cd `mktemp -d`";
      cp = "cp -iv";
      ln = "ln -v";
      mkdir = "mkdir -vp";
      mv = "mv -iv";
      rm = lib.mkMerge [
        (lib.mkIf pkgs.stdenv.targetPlatform.isDarwin "rm -v")
        (lib.mkIf (!pkgs.stdenv.targetPlatform.isDarwin) "rm -Iv")
      ];
      dh = "du -h";
      df = "df -h";
      su = "sudo -E su -m";
      sysu = "systemctl --user";
      jnsu = "journalctl --user";
      zreload = "export ZSH_RELOADING_SHELL=1; source $ZDOTDIR/.zshenv; source $ZDOTDIR/.zshrc; unset ZSH_RELOADING_SHELL";

      urldecode = "python3 -c 'import sys, urllib.parse as ul; print(ul.unquote_plus(sys.stdin.read()))'";
      urlencode = "python3 -c 'import sys, urllib.parse as ul; print(ul.quote_plus(sys.stdin.read()))'";

      # Git auto-commit with Claude
      gcauto = ''git commit -m "$(claude -p "Look at the staged git changes and create a summarizing git commit title. Only respond with the title and no affirmation.")"'';
    };
  };

}

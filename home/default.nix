{
  pkgs,
  username,
  config,
  ...
}:
{
  # import sub modules
  imports = [
    ./core.nix
    ./shell.nix
    ./spacemacs.nix

    ./alacritty.nix
    ./docker.nix
    ./git.nix
    ./neovim.nix
    ./python.nix
    ./starship.nix
    ./tmux.nix
    ./wezterm.nix

    # Configs
    ./config.nix
    ./gpg.nix

    # Scripts
    ./scripts
  ];

  # Home Manager needs a bit of information about you and the
  # paths it should manage.
  home = {
    username = username;
    homeDirectory = "/Users/${username}";

    # This value determines the Home Manager release that your
    # configuration is compatible with. This helps avoid breakage
    # when a new Home Manager release introduces backwards
    # incompatible changes.
    #
    # You can update Home Manager without changing this value. See
    # the Home Manager release notes for a list of state version
    # changes in each release.
    stateVersion = "25.05";

    sessionVariables = {
      VISUAL = "emacsclient -a=vim";
      MANPAGER = "sh -c 'col -b | bat -l man -p'";
      MANROFFOPT = "-c";

      SAM_CLI_TELEMETRY = "0";

      BUNBIN = "$HOME/.bun/bin";
      PNPM_HOME = "$HOME/Library/pnpm";
      NODE_PATH = "$HOME/.bun/install/global/node_modules";
    };

    sessionPath = [
      "$BUNBIN"
      "${config.home.sessionVariables.PNPM_HOME}"
      "$HOME/.local/bin"
      "$HOME/.cargo/bin"
    ];

  };

  # Let Home Manager install and manage itself.
  programs.home-manager.enable = true;
}

{ pkgs, username, ... }:
{
  # import sub modules
  imports = [
    ./core.nix
    ./shell.nix
    ./alacritty.nix
    ./git.nix
    ./go.nix
    ./spacemacs.nix
    ./starship.nix
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
    stateVersion = "24.05";

    file.".gnupg/gpg-agent.conf".text = ''
      max-cache-ttl 18000
      default-cache-ttl 18000
      pinentry-program ${pkgs.pinentry_mac}/bin/pinentry-mac
      enable-ssh-support
    '';
  };

  # Let Home Manager install and manage itself.
  programs.home-manager.enable = true;
}

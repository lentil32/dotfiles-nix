{
  pkgs,
  username,
  hostname,
  uid,
  ...
}@args:
#############################################################
#
#  Host & Users configuration
#
#############################################################
{
  networking.hostName = hostname;
  networking.computerName = hostname;
  system.defaults.smb.NetBIOSName = hostname;

  # Define a user account. Don't forget to set a password with ‘passwd’.
  users.knownUsers = [ username ];
  users.users."${username}" = {
    home = "/Users/${username}";
    description = username;
    shell = pkgs.zsh;
    uid = uid;
  };
  system.primaryUser = username;

  nix.settings.trusted-users = [ username ];
}

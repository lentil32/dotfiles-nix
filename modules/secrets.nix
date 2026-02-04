{
  config,
  lib,
  username,
  ...
}:
let
  userHome = config.users.users.${username}.home or "/Users/${username}";
  userGroup = config.users.users.${username}.group or "staff";
  secretsFile = ../secrets/nix-access-tokens.yaml;
  ghHostsFile = ../secrets/gh-hosts.yaml;
  maintenanceFile = ../secrets/git-maintenance.yaml;
  sshConfigFile = ../secrets/ssh-config.yaml;
  hasSecretsFile = builtins.pathExists secretsFile;
  hasGhHostsFile = builtins.pathExists ghHostsFile;
  hasMaintenanceFile = builtins.pathExists maintenanceFile;
  hasSshConfigFile = builtins.pathExists sshConfigFile;
in
{
  sops = lib.mkMerge [
    {
      age.keyFile = "${userHome}/.config/sops/age/keys.txt";
      age.sshKeyPaths = [ ];
      gnupg.sshKeyPaths = [ ];
    }
    (lib.mkIf hasSecretsFile {
      defaultSopsFile = secretsFile;
      secrets."nix-access-tokens" = {
        path = "${userHome}/.config/nix/nix.conf.local";
        owner = username;
        group = userGroup;
        mode = "0600";
      };
    })
    (lib.mkIf hasGhHostsFile {
      secrets."gh-hosts" = {
        sopsFile = ghHostsFile;
        path = "${userHome}/.config/gh/hosts.yml";
        owner = username;
        group = userGroup;
        mode = "0600";
      };
    })
    (lib.mkIf hasMaintenanceFile {
      secrets."git-maintenance" = {
        sopsFile = maintenanceFile;
        path = "${userHome}/.config/git/maintenance.inc";
        owner = username;
        group = userGroup;
        mode = "0600";
      };
    })
    (lib.mkIf hasSshConfigFile {
      secrets."ssh-config" = {
        sopsFile = sshConfigFile;
        path = "${userHome}/.ssh/config.private";
        owner = username;
        group = userGroup;
        mode = "0600";
      };
    })
  ];
}

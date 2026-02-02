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
  hasSecretsFile = builtins.pathExists secretsFile;
in
{
  sops = lib.mkMerge [
    {
      age.keyFile = "${userHome}/.config/sops/age/keys.txt";
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
  ];
}

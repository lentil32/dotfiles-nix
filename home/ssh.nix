{ config, lib, ... }:
let
  homeDir = config.home.homeDirectory;
  privateConfigFile = "${homeDir}/.ssh/config.private";
  privateConfigSecret = ../secrets/ssh-config.yaml;
  hasPrivateConfig = builtins.pathExists privateConfigSecret;
  extraConfigLines = [
    "Include ${homeDir}/.colima/ssh_config"
  ]
  ++ lib.optional hasPrivateConfig "Include ${privateConfigFile}";
in
{
  programs.ssh = {
    enable = true;
    enableDefaultConfig = false;
    extraConfig = lib.concatStringsSep "\n" extraConfigLines;
    matchBlocks = {
      "github.com" = {
        hostname = "ssh.github.com";
        user = "git";
        port = 443;
        identityFile = [ "~/.ssh/github-personal" ];
        identitiesOnly = true;
        addKeysToAgent = "yes";
        extraOptions = {
          PreferredAuthentications = "publickey";
          UseKeychain = "yes";
          ControlMaster = "auto";
          ControlPersist = "10m";
          ControlPath = "~/.ssh/cm-%C";
        };
      };
      "*" = { };
    };
  };
}

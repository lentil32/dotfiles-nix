{ ... }:
{
  programs.ssh = {
    enable = true;
    matchBlocks = {
      "github.com" = {
        hostname = "github.com";
        user = "git";
        extraOptions = {
          ControlMaster = "auto";
          ControlPersist = "10m";
          ControlPath = "~/.ssh/cm-%C";
        };
      };
    };
  };
}

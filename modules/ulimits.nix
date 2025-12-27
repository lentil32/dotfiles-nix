{
  config,
  lib,
  pkgs,
  ...
}:

{
  # Set system-wide ulimits using launchd
  launchd.daemons.ulimit-setter = {
    serviceConfig = {
      Label = "org.nixos.ulimit-setter";
      ProgramArguments = [
        "/bin/sh"
        "-c"
        "/bin/launchctl limit maxfiles 1048575 1048575"
      ];
      RunAtLoad = true;
      StandardOutPath = "/tmp/ulimit-setter.log";
      StandardErrorPath = "/tmp/ulimit-setter.err";
    };
  };

  # Set user-specific ulimits
  launchd.user.agents.ulimit-setter = {
    serviceConfig = {
      Label = "org.nixos.ulimit-setter-user";
      ProgramArguments = [
        "/bin/sh"
        "-c"
        "/bin/launchctl limit maxfiles 1048575 1048575"
      ];
      RunAtLoad = true;
      StandardOutPath = "/tmp/ulimit-setter-user.log";
      StandardErrorPath = "/tmp/ulimit-setter-user.err";
    };
  };
}

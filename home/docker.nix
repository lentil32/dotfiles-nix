{ pkgs, pkgs-unstable, ... }:
{
  # Docker-related packages
  home.packages = [
    pkgs-unstable.colima # Docker runtime with minimal setup
    pkgs.docker # Docker CLI and engine
    pkgs.hadolint # Docker linter
    pkgs.docker-buildx # Docker Buildx for multi-platform builds
  ];

  # Set up the Docker CLI plugin by symlinking the buildx binary
  # https://github.com/abiosoft/colima/discussions/273#discussioncomment-6453101
  home.file.".docker/cli-plugins/docker-buildx" = {
    source = "${pkgs.docker-buildx}/bin/docker-buildx";
  };
}

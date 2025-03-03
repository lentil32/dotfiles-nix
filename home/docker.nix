{ pkgs, ... }:
{
  # Docker-related packages
  home.packages = with pkgs; [
    colima # Docker runtime with minimal setup
    docker # Docker CLI and engine
    hadolint # Docker linter
    docker-buildx # Docker Buildx for multi-platform builds
  ];

  # Set up the Docker CLI plugin by symlinking the buildx binary
  # https://github.com/abiosoft/colima/discussions/273#discussioncomment-6453101
  home.file.".docker/cli-plugins/docker-buildx" = {
    source = "${pkgs.docker-buildx}/bin/docker-buildx";
  };
}

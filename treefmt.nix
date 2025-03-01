{
  projectRootFile = "flake.nix"; # Defines the root of your project
  programs.nixpkgs-fmt.enable = true; # Enables formatting for Nix files

  # Exclude files that don’t need formatting
  settings.excludes = [
    "Makefile"
    "*.org"
    ".gitignore"
    "flake.lock"
  ];
}

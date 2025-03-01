{
  projectRootFile = "flake.nix";
  programs.nixfmt.enable = true;

  # Exclude files that don’t need formatting
  settings.excludes = [
    "Makefile"
    "*.org"
    ".gitignore"
    "flake.lock"
  ];
}

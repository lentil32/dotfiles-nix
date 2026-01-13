{
  projectRootFile = "flake.nix";
  programs.nixfmt.enable = true;
  programs.rustfmt.enable = true;
  programs.stylua.enable = true;

  # Exclude files that don’t need formatting
  settings.excludes = [
    "Makefile"
    "*.org"
    ".gitignore"
    "flake.lock"
  ];
}

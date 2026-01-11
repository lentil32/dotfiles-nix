{
  projectRootFile = "flake.nix";
  programs.nixfmt.enable = true;
  programs.stylua.enable = true;
  programs.stylua.settings = {
    indent_type = "Spaces";
    indent_width = 2;
  };

  # Exclude files that don’t need formatting
  settings.excludes = [
    "Makefile"
    "*.org"
    ".gitignore"
    "flake.lock"
  ];
}

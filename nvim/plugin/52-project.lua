require("project").setup({
  use_lsp = true,
  patterns = { ".git", "package.json", "Cargo.toml", "flake.nix", "Makefile" },
  silent_chdir = true,
  show_hidden = true,
})

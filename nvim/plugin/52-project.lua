local ok, project = pcall(require, "project")
if not ok then
  return
end

project.setup({
  use_lsp = true,
  patterns = { ".git", "package.json", "Cargo.toml", "flake.nix", "Makefile" },
  silent_chdir = true,
  show_hidden = true,
})

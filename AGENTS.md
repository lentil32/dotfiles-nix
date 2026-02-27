## Build, Test, and Development Commands
```bash
make               # Build and apply the system configuration (darwin)
make darwin-debug  # Build with verbose tracing
make fmt           # Format Nix files via nixfmt (treefmt)
make check         # Run flake checks
make update-flake  # Update flake.lock
make deploy-flake  # Update flake.lock and rebuild
make gc            # Wipe old system generations and GC unused store paths
make clean         # Remove the ./result symlink
```

## Agent Notes
- See `.tmp/` if library references needed. There are cloned libraries we are using in this repository. Treat these directories as readonly.

List:
- lualine.nvim
- lze
- lzextras
- nixCats-nvim
- nvim-oxi
- oil.nvim
- snacks.nvim
- ...


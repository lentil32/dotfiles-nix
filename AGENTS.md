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

See `.tmp/` if library references needed. There are cloned libraries we are using in this repository. Treat this directory as readonly.
The cloned library repository directories are suffixed with `-repo`.

List:

- nvim-oxi-repo/
- oil.nvim-repo/
- snacks.nvim-repo/
- sidekick.nvim-repo/
- ...

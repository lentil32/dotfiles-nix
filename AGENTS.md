# Repository Guidelines

This repository contains a macOS setup managed with Nix flakes, nix-darwin, and home-manager. Changes typically touch Nix modules, host-specific configuration, or editor tooling.

## Project Structure & Module Organization
- `flake.nix` / `flake.lock`: entry point and pinned inputs for the system.
- `modules/`: nix-darwin system modules (e.g., `system.nix`, `nix-core.nix`).
- `modules/<hostname>/`: per-host configuration such as `apps.nix` (e.g., `modules/lentil32-MacBookPro/apps.nix`).
- `home/`: home-manager modules; `home/default.nix` imports program configs like `home/git.nix`, `home/shell.nix`.
- `nvim/`: Neovim config in Lua (`nvim/init.lua`, `nvim/lua/`).
- `treefmt.nix`: formatter configuration; `Makefile` provides common tasks.

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

## Coding Style & Naming Conventions
- Nix formatting is handled by `nix fmt`/`make fmt` (nixfmt via treefmt). Keep changes `nixfmt`-clean.
- Follow existing file patterns: system modules in `modules/*.nix`, host overrides in `modules/<hostname>/`, user configs in `home/*.nix`.
- Lua in `nvim/` follows the project’s existing style.
- DO NOT care about backward compatibility unless user mentioned to do so.

## Testing Guidelines
- There is no dedicated test suite; the primary check is `nix flake check` (`make check`).
- For system changes, validate by rebuilding (`make` or `make darwin-debug`) on macOS.

## Commit & Pull Request Guidelines
- Commit messages are short and action-oriented; conventional prefixes appear often (e.g., `feat:`, `refactor:`, `migrate:`) but are not strictly enforced.

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


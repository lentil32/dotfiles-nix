# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

Declarative macOS configuration using nix-darwin and home-manager with Nix flakes.

## Common Commands

```bash
make              # Build and apply full system configuration
make darwin-debug # Build with verbose output and tracing
make fmt          # Format all Nix files (uses nixfmt via treefmt-nix)
make check        # Run flake checks
make gc           # Garbage collect (removes generations older than 7 days)
make gc-all       # Full garbage collection
make update-flake # Update flake.lock
make deploy-flake # Update flake.lock and rebuild
```

## Architecture

### Flake Structure

`flake.nix` is the entry point. It builds `darwinConfigurations` for each host machine defined in the `machines` attrset. Each configuration combines:

1. **Base darwin modules** (`modules/*.nix`) - System-wide nix-darwin settings
2. **Host-specific modules** (`modules/<hostname>/*.nix`) - Per-machine apps/homebrew config
3. **Home Manager** (`home/`) - User-level dotfiles and packages

### Key Directories

- `modules/` - nix-darwin system modules
  - `nix-core.nix` - Nix settings (flakes, gc, unfree)
  - `system.nix` - macOS defaults, fonts, keyboard, security
  - `host-users.nix` - User account configuration
  - `services/` - System services (aerospace window manager)
  - `<hostname>/` - Host-specific modules (apps, homebrew casks/brews)

- `home/` - home-manager modules
  - `default.nix` - Entry point, imports all home modules
  - `core.nix` - Development tools and CLI packages
  - `shell.nix` - Zsh configuration
  - Other files configure specific programs (git, tmux, starship, etc.)

- `overlays/` - Nixpkgs overlays

### Adding Software

- **Nix packages**: Add to `home/core.nix` for user packages or `modules/<hostname>/apps.nix` for system packages
- **Homebrew casks/brews**: Add to `modules/<hostname>/apps.nix` under `homebrew.casks` or `homebrew.brews`
- **Mac App Store apps**: Add to `homebrew.masApps` (requires prior manual install)

### Multi-Host Support

The `machines` attrset in `flake.nix` defines host configurations. Each host can have:
- Custom `system` architecture
- Custom `uid`
- Host-specific modules via `extraModulesDir`

The `listNixModules` helper auto-discovers all `.nix` files in a host's module directory.

### Overlays

Active overlays (applied via `nixpkgsConfig`):
- `nix-darwin-emacs` - Custom Emacs builds
- `rust-overlay` - Rust toolchain with WASM target
- `ghostty` - Ghostty terminal
- `pkgs-unstable` - Access to nixpkgs-unstable packages

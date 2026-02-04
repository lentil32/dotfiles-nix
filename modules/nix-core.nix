{ pkgs, lib, ... }:
let
  binaryCaches = [
    "https://cache.nixos.org"
    "https://nix-community.cachix.org"
    "https://ghostty.cachix.org"
    "https://neovim-nightly.cachix.org"
  ];
  trustedPublicKeys = [
    "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY="
    "nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs="
    "ghostty.cachix.org-1:QB389yTa6gTyneehvqG58y0WnHjQOqgnA+wBnpWWxns="
    "neovim-nightly.cachix.org-1:feIoInHRevVEplgdZvQDjhp11kYASYCE2NGY9hNrwxY="
  ];
in
{
  # enable flakes globally
  nix.settings.experimental-features = [
    "nix-command"
    "flakes"
  ];

  # Enable trusted binary caches globally (not just per-flake).
  nix.settings.substituters = binaryCaches;
  nix.settings.trusted-public-keys = trustedPublicKeys;

  # Allow unfree packages
  nixpkgs.config.allowUnfree = true;

  nix.package = pkgs.nix;

  # do garbage collection weekly to keep disk usage low
  nix.gc = {
    automatic = lib.mkDefault true;
    options = lib.mkDefault "--delete-older-than 30d";
  };

  nix.optimise.automatic = true;

  # Performance tuning: keep more paths, build in parallel, and download more substitutes.
  nix.settings.max-jobs = lib.mkDefault "auto";
  nix.settings.cores = lib.mkDefault 0;
  nix.settings.http-connections = lib.mkDefault 50;
}

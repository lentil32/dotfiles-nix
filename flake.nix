{
  description = "Declarative macOS configuration with nix-darwin + Home Manager";

  # the nixConfig here only affects the flake itself, not the system configuration!
  nixConfig = {
    substituters = [
      "https://cache.nixos.org"
      "https://nix-community.cachix.org"
    ];
    trusted-public-keys = [
      "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY="
      "nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs="
    ];
  };

  inputs = {
    nixpkgs-darwin.url = "github:nixos/nixpkgs/nixpkgs-24.11-darwin";
    # home-manager, used for managing user configuration
    home-manager = {
      url = "github:nix-community/home-manager/release-24.11";
      # The `follows` keyword in inputs is used for inheritance.
      # Here, `inputs.nixpkgs` of home-manager is kept consistent with the `inputs.nixpkgs` of the current flake,
      # to avoid problems caused by different versions of nixpkgs dependencies.
      inputs.nixpkgs.follows = "nixpkgs-darwin";
    };
    darwin = {
      url = "github:LnL7/nix-darwin/master";
      inputs.nixpkgs.follows = "nixpkgs-darwin";
    };
    darwin-emacs = {
      url = "github:nix-giant/nix-darwin-emacs";
      inputs.nixpkgs.follows = "nixpkgs-darwin";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs-darwin";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      darwin,
      home-manager,
      darwin-emacs,
      rust-overlay,
      ...
    }@inputs:
    let
      username = "starush";
      useremail = "lentil32@icloud.com";
      system = "aarch64-darwin"; # aarch64-darwin or x86_64-darwin
      hostname = "lentil32-MacBookPro";
      uid = 502;
      nixpkgsConfig = {
        overlays = [
          darwin-emacs.overlays.emacs
          rust-overlay.overlays.default
        ];
      };

      specialArgs = inputs // {
        inherit
          username
          useremail
          hostname
          uid
          ;
      };
    in
    {
      darwinConfigurations."${hostname}" = darwin.lib.darwinSystem {
        inherit system specialArgs;
        modules = [
          ./modules/nix-core.nix
          ./modules/system.nix
          ./modules/apps.nix
          ./modules/host-users.nix
          home-manager.darwinModules.home-manager
          {
            nixpkgs = nixpkgsConfig;
            home-manager.useGlobalPkgs = true;
            home-manager.useUserPackages = true;
            home-manager.extraSpecialArgs = specialArgs;
            home-manager.users.${username} = import ./home;
          }
        ];
      };
      # nix code formatter
      formatter.${system} = nixpkgs.legacyPackages.${system}.nixfmt;
    };
}

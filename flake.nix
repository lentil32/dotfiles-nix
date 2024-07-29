{
  description = "Declarative macOS configuration with nix-darwin + Home Manager";

  # the nixConfig here only affects the flake itself, not the system configuration!
  nixConfig = {
    substituters = [
      "https://cache.nixos.org"
      "https://nix-community.cachix.org"
    ];
    trusted-public-keys = [ "nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs=" ];
  };

  inputs = {
    nixpkgs-darwin.url = "github:nixos/nixpkgs/nixpkgs-24.05-darwin";
    # home-manager, used for managing user configuration
    home-manager = {
      url = "github:nix-community/home-manager/release-24.05";
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
      url = "github:c4710n/nix-darwin-emacs";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    emacs.url = "github:cmacrae/emacs";
  };

  outputs =
    {
      self,
      nixpkgs,
      darwin,
      darwin-emacs,
      home-manager,
      ...
    }@inputs:
    let
      username = "starush";
      useremail = "lentil32@icloud.com";
      system = "aarch64-darwin"; # aarch64-darwin or x86_64-darwin
      hostname = "lentil32-MacBookPro";

      specialArgs = inputs // {
        inherit username useremail hostname;
      };
    in
    {
      darwinConfigurations."${hostname}" = darwin.lib.darwinSystem {
        inherit system specialArgs;
        modules = [

          {

            nixpkgs.overlays = [ darwin-emacs.overlays.emacs ];
          }
          ./modules/nix-core.nix
          ./modules/system.nix
          ./modules/apps.nix
          ./modules/host-users.nix
          home-manager.darwinModules.home-manager
          {
            home-manager.useGlobalPkgs = true;
            home-manager.useUserPackages = true;
            home-manager.extraSpecialArgs = specialArgs;
            home-manager.users.${username} = import ./home;
          }
        ];
      };
      # nix code formatter
      formatter.${system} = nixpkgs.legacyPackages.${system}.nixfmt-rfc-style;
    };
}

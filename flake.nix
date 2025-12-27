{
  description = "Declarative macOS configuration with nix-darwin + Home Manager";

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
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-25.05-darwin";
    nixpkgs-unstable.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    home-manager = {
      url = "github:nix-community/home-manager/release-25.05";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    nix-darwin = {
      url = "github:LnL7/nix-darwin/nix-darwin-25.05";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    nix-darwin-emacs = {
      url = "github:lentil32/nix-darwin-emacs";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    rust-overlay.url = "github:oxalica/rust-overlay";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    flake-utils.url = "github:numtide/flake-utils";

    ghostty = {
      url = "github:ghostty-org/ghostty";
    };

    # Homebrew management
    nix-homebrew.url = "github:zhaofengli/nix-homebrew";

    # Homebrew taps (declarative)
    homebrew-core = {
      url = "github:homebrew/homebrew-core";
      flake = false;
    };
    homebrew-cask = {
      url = "github:homebrew/homebrew-cask";
      flake = false;
    };
    homebrew-services = {
      url = "github:homebrew/homebrew-services";
      flake = false;
    };
    homebrew-pear = {
      url = "github:pear-devs/homebrew-pear";
      flake = false;
    };
    homebrew-sst = {
      url = "github:sst/homebrew-tap";
      flake = false;
    };
    homebrew-supabase = {
      url = "github:supabase/homebrew-tap";
      flake = false;
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      nixpkgs-unstable,
      nix-darwin,
      home-manager,
      nix-darwin-emacs,
      rust-overlay,
      treefmt-nix,
      flake-utils,
      ghostty,
      nix-homebrew,
      homebrew-core,
      homebrew-cask,
      homebrew-services,
      homebrew-pear,
      homebrew-sst,
      homebrew-supabase,
      ...
    }@inputs:

    let
      # ---------------- Common user data ----------------
      username = "starush";
      useremail = "lentil32@icloud.com";

      # ---------------- Per-host declarations ----------------
      macBookProHost = "lentil32-MacBookPro";
      macMiniM1Host = "lentil32-MacMiniM1";

      machines = {
        ${macBookProHost} = {
          system = "aarch64-darwin";
          hostname = macBookProHost;
          uid = 502;
          extraModulesDir = ./. + "/modules/${macBookProHost}";
        };

        # ${macMiniM1Host} = {
        #   system      = "aarch64-darwin";
        #   hostname    = macMiniM1Host;
        #   uid         = 500;
        # };
      };

      defaultMachine = machines.${macBookProHost};

      nixpkgsConfig = {
        overlays = with inputs; [
          nix-darwin-emacs.overlays.emacs
          rust-overlay.overlays.default
          ghostty.overlays.default
          (final: prev: {
            pkgs-unstable = nixpkgs-unstable.legacyPackages.${prev.system};
          })
        ];
      };

      # Return a list of <dir>/<file>.nix for all regular *.nix files in <dir>.
      listNixModules =
        dir:
        builtins.map (name: dir + "/${name}") (
          builtins.filter (
            n: (builtins.readDir dir).${n} == "regular" && builtins.match ".*\\.nix" n != null
          ) (builtins.attrNames (builtins.readDir dir))
        );

      treefmtEval = system: treefmt-nix.lib.evalModule nixpkgs.legacyPackages.${system} ./treefmt.nix;

      pkgs = nixpkgs.legacyPackages.${defaultMachine.system};
      pkgs-unstable = nixpkgs-unstable.legacyPackages.${defaultMachine.system};

      baseSpecialArgs = inputs // {
        inherit pkgs-unstable username useremail ghostty;
      };

    in
    {
      # ╔════════════════════════════════════════════════════════════╗
      # ║      Build a darwinConfiguration for every machine        ║
      # ╚════════════════════════════════════════════════════════════╝
      darwinConfigurations = builtins.mapAttrs (
        name: machine:
        let
          system = machine.system;
          specialArgs = baseSpecialArgs // {
            inherit (machine) hostname uid system;
          };
        in
        nix-darwin.lib.darwinSystem {
          inherit system specialArgs;

          # base + host-specific + trailing common modules
          modules =
            [
              ./modules/nix-core.nix
              ./modules/system.nix
              ./modules/ulimits.nix
              ./modules/services/aerospace.nix
            ]
            ++ (
              if machine ? extraModulesDir then
                listNixModules machine.extraModulesDir
              else
                (machine.extraModules or [ ])
            )
            ++ [
              ./modules/host-users.nix
              home-manager.darwinModules.home-manager
              {
                nixpkgs = nixpkgsConfig;
                home-manager.useGlobalPkgs = true;
                home-manager.useUserPackages = true;
                home-manager.extraSpecialArgs = specialArgs;
                home-manager.users.${username} = import ./home;
              }
              # Homebrew management
              nix-homebrew.darwinModules.nix-homebrew
              {
                nix-homebrew = {
                  enable = true;
                  user = username;
                  mutableTaps = false;
                  taps = {
                    "homebrew/homebrew-core" = homebrew-core;
                    "homebrew/homebrew-cask" = homebrew-cask;
                    "homebrew/homebrew-services" = homebrew-services;
                    "pear-devs/homebrew-pear" = homebrew-pear;
                    "sst/homebrew-tap" = homebrew-sst;
                    "supabase/homebrew-tap" = homebrew-supabase;
                  };
                };
              }
              # Sync homebrew.taps with nix-homebrew taps
              (
                { config, ... }:
                {
                  homebrew.taps = builtins.attrNames config.nix-homebrew.taps;
                }
              )
            ];
        }
      ) machines;

      formatter.${defaultMachine.system} = (treefmtEval defaultMachine.system).config.build.wrapper;

      checks.${defaultMachine.system} = {
        formatting = (treefmtEval defaultMachine.system).config.build.check self;
      };
    };
}

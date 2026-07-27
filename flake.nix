{
  description = "Declarative macOS configuration with nix-darwin + Home Manager";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-25.11-darwin";
    nixpkgs-unstable.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    home-manager = {
      url = "github:nix-community/home-manager/release-25.11";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    nix-darwin = {
      url = "github:LnL7/nix-darwin/nix-darwin-25.11";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    sops-nix = {
      url = "github:Mic92/sops-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    rust-overlay.url = "github:oxalica/rust-overlay";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    claude-code-nix = {
      url = "github:sadjow/claude-code-nix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-utils.follows = "flake-utils";
    };
    codex-cli-nix = {
      url = "github:sadjow/codex-cli-nix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-utils.follows = "flake-utils";
    };
    gemini-cli-nix = {
      url = "github:sadjow/gemini-cli-nix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-utils.follows = "flake-utils";
    };

    nixCats.url = "github:BirdeeHub/nixCats-nvim";

    # Homebrew management. nix-homebrew still pins 6.0.12, while current
    # core/cask taps require install-step and cask DSL additions from 6.0.13.
    # Drop this override once nix-homebrew itself pins 6.0.13 or newer.
    brew-src = {
      url = "github:Homebrew/brew/6.0.13";
      flake = false;
    };
    nix-homebrew = {
      url = "github:zhaofengli/nix-homebrew";
      inputs.brew-src.follows = "brew-src";
    };

    nur = {
      url = "github:nix-community/NUR";
      inputs.nixpkgs.follows = "nixpkgs";
    };

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
    homebrew-anomalyco = {
      url = "github:anomalyco/homebrew-tap";
      flake = false;
    };
    homebrew-wordbricks = {
      url = "github:wordbricks/homebrew-tap";
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
      sops-nix,
      rust-overlay,
      treefmt-nix,
      crane,
      flake-utils,
      claude-code-nix,
      codex-cli-nix,
      gemini-cli-nix,
      brew-src,
      nix-homebrew,
      nur,
      homebrew-core,
      homebrew-cask,
      homebrew-services,
      homebrew-pear,
      homebrew-anomalyco,
      homebrew-wordbricks,
      nixCats,
      ...
    }@inputs:

    let
      # ---------------- Common user data ----------------
      useremail = "lentil32@icloud.com";

      # ---------------- Per-host declarations ----------------
      m5ProHost = "lentil32-M5Pro";
      m5ProModulesDir = ./. + "/modules/${m5ProHost}";
      machines = {
        ${m5ProHost} = {
          system = "aarch64-darwin";
          hostname = m5ProHost;
          username = "lentil32";
          uid = 501;
          extraModulesDir = m5ProModulesDir;
        };

        # ${macMiniM1Host} = {
        #   system      = "aarch64-darwin";
        #   hostname    = macMiniM1Host;
        #   uid         = 500;
        # };
      };

      defaultMachine = machines.${m5ProHost};

      pkgsUnstableFor =
        system:
        import nixpkgs-unstable {
          inherit system;
        };

      nixpkgsConfig = {
        overlays = [
          rust-overlay.overlays.default
          nur.overlays.default
          (_final: prev: {
            pkgs-unstable = pkgsUnstableFor prev.stdenv.hostPlatform.system;
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
      devShellPkgs = import nixpkgs {
        inherit (defaultMachine) system;
        inherit (nixpkgsConfig) overlays;
      };
      pkgs-unstable = pkgsUnstableFor defaultMachine.system;
      craneLib = crane.mkLib pkgs;
      rustSrc = craneLib.cleanCargoSource ./nvim/rust;
      rustLockHashes = import ./nvim/rust/lock-hashes.nix;
      nvimOxiSourceHashes = rustLockHashes.bySource;
      commonArgs = {
        src = rustSrc;
        cargoLock = ./nvim/rust/Cargo.lock;
        outputHashes = nvimOxiSourceHashes;
        cargoExtraArgs = "--locked --workspace";
        pname = "nvim-rust-workspace-check";
        version = "0.0.0";
      };
      cargoArtifacts = craneLib.buildDepsOnly commonArgs;
      rustToolchain = devShellPkgs.rust-bin.stable.latest.default.override {
        targets = [ "wasm32-unknown-unknown" ];
        extensions = [
          "clippy"
          "rust-analyzer"
          "rust-src"
        ];
      };
      rustDevShell = devShellPkgs.mkShell {
        packages = [
          rustToolchain
          devShellPkgs.cargo-insta
          devShellPkgs.cargo-nextest
          devShellPkgs.clang
          devShellPkgs.just
        ];
        nativeBuildInputs = [
          devShellPkgs.cmake
          devShellPkgs.gnumake
          devShellPkgs.ninja
          devShellPkgs.pkg-config
        ];
        buildInputs = [
          devShellPkgs.libiconv
        ];
      };

      baseSpecialArgs = inputs // {
        inherit
          inputs
          pkgs-unstable
          useremail
          ;
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
            inherit (machine)
              hostname
              username
              uid
              system
              ;
          };
        in
        nix-darwin.lib.darwinSystem {
          inherit system specialArgs;

          # base + host-specific + trailing common modules
          modules = [
            ./modules/nix-core.nix
            ./modules/system.nix
            ./modules/ulimits.nix
            sops-nix.darwinModules.sops
            ./modules/secrets.nix
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
              home-manager.users.${machine.username} = import ./home;
            }
            # Homebrew management
            nix-homebrew.darwinModules.nix-homebrew
            {
              nix-homebrew = {
                enable = true;
                user = machine.username;
                mutableTaps = false;
                # Keep the patched package metadata aligned with the overridden
                # source; nix-homebrew otherwise labels it with its own 6.0.12.
                package = brew-src // {
                  name = "brew-6.0.13";
                  version = "6.0.13";
                };
                trust.casks = [ "pear-devs/pear/pear-desktop" ];
                # In Homebrew, the repo part of all taps always have homebrew- prepended.
                taps = {
                  "homebrew/homebrew-core" = homebrew-core;
                  "homebrew/homebrew-cask" = homebrew-cask;
                  "homebrew/homebrew-services" = homebrew-services;
                  "pear-devs/homebrew-pear" = homebrew-pear;
                  "anomalyco/homebrew-tap" = homebrew-anomalyco;
                  "wordbricks/homebrew-tap" = homebrew-wordbricks;
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

      devShells.${defaultMachine.system}.nvim-rust = rustDevShell;

      checks.${defaultMachine.system} = {
        formatting = (treefmtEval defaultMachine.system).config.build.check self;
        nvim-rust-check = craneLib.mkCargoDerivation (
          commonArgs
          // {
            inherit cargoArtifacts;
            buildPhaseCargoCommand = "cargo check ${commonArgs.cargoExtraArgs}";
            doCheck = false;
          }
        );
      };
    };
}

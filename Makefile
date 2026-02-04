.PHONY: darwin darwin-debug nvim update-emacs update-flake update-all deploy-emacs deploy-flake deploy-all history gc fmt check clean help

hostname := $(shell hostname)
user := $(shell whoami)

############################################################################
#
#  Darwin related commands
#
############################################################################

darwin:
	nix build .#darwinConfigurations.${hostname}.system \
	  --extra-experimental-features 'nix-command flakes'

	sudo ./result/sw/bin/darwin-rebuild switch --flake .#${hostname}

# darwin-debug: darwin-set-proxy
darwin-debug:
	nix build .#darwinConfigurations.${hostname}.system --show-trace --verbose \
	  --extra-experimental-features 'nix-command flakes'

	sudo ./result/sw/bin/darwin-rebuild switch --flake .#${hostname} --show-trace --verbose

############################################################################
#
#  nix related commands
#
############################################################################

nvim:
	nix build .#darwinConfigurations.${hostname}.config.home-manager.users.${user}.home.activationPackage \
	  --extra-experimental-features 'nix-command flakes'

	./result/activate

update-emacs:
	gh repo sync lentil32/nix-darwin-emacs --source nix-giant/nix-darwin-emacs

update-flake:
	nix flake update

update-all: update-flake

deploy-emacs: update-emacs darwin
deploy-flake: update-flake darwin
deploy-all: update-all darwin

history:
	nix profile history --profile /nix/var/nix/profiles/system

gc:
	sudo nix-collect-garbage -d

fmt:
	nix fmt

check:
	nix flake check

clean:
	rm -rf result

help:
	@echo "darwin       - Build and apply system configuration"
	@echo "darwin-debug - Build with verbose output"
	@echo "nvim         - Build and apply home-manager activation (Neovim config)"
	@echo "update-flake - Update flake.lock"
	@echo "deploy-flake - Update flake.lock and rebuild"
	@echo "gc           - Full garbage collection"
	@echo "fmt          - Format Nix files"
	@echo "check        - Run flake checks"
	@echo "clean        - Remove result symlink"

.PHONY: darwin darwin-debug nvim nvim-profile update-flake update-all deploy-flake deploy-all history gc fmt check check-lua clean help

hostname := $(shell hostname)
user := $(shell whoami)
emmylua_config := .emmyrc.json
emmylua_workspace := nvim/lua/myLuaConf

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

nvim-profile:
	bash ./home/scripts/nvim-profile.sh $(ARGS)

update-flake:
	nix flake update

update-all: update-flake

deploy-flake: update-flake darwin
deploy-all: update-all darwin

history:
	nix profile history --profile /nix/var/nix/profiles/system

gc:
	sudo nix-collect-garbage -d

fmt:
	nix fmt

check:
	$(MAKE) check-lua
	nix flake check

check-lua:
	emmylua_check -c $(emmylua_config) $(emmylua_workspace)

clean:
	rm -rf result

help:
	@echo "darwin       - Build and apply system configuration"
	@echo "darwin-debug - Build with verbose output"
	@echo "nvim         - Build and apply home-manager activation (Neovim config)"
	@echo "nvim-profile - Profile Neovim startup (pass args via ARGS='...')"
	@echo "update-flake - Update flake.lock"
	@echo "deploy-flake - Update flake.lock and rebuild"
	@echo "gc           - Full garbage collection"
	@echo "fmt          - Format Nix files"
	@echo "check        - Run EmmyLua and flake checks"
	@echo "check-lua    - Run EmmyLua checks"
	@echo "clean        - Remove result symlink"

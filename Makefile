.PHONY: darwin darwin-debug update-emacs update update-all history gc fmt clean

hostname := $(shell hostname)

############################################################################
#
#  Darwin related commands
#
############################################################################

darwin:
	nix build .#darwinConfigurations.${hostname}.system \
	  --extra-experimental-features 'nix-command flakes'

	./result/sw/bin/darwin-rebuild switch --flake .#${hostname}

# darwin-debug: darwin-set-proxy
darwin-debug:
	nix build .#darwinConfigurations.${hostname}.system --show-trace --verbose \
	  --extra-experimental-features 'nix-command flakes'

	./result/sw/bin/darwin-rebuild switch --flake .#${hostname} --show-trace --verbose

############################################################################
#
#  nix related commands
#
############################################################################

update-emacs:
	gh repo sync lentil32/nix-darwin-emacs --source nix-giant/nix-darwin-emacs

update-aider:
	./scripts/update-aider.sh ./overlays/aider-latest-flake.nix

update-flake:
	nix flake update

update-all: update-emacs update-aider update-flake

deploy-emacs: update-emacs darwin
deploy-aider: update-aider darwin
deploy-flake: update-flake darwin
deploy-all: update-all darwin

history:
	nix profile history --profile /nix/var/nix/profiles/system

gc:
# remove all generations older than 7 days
	sudo nix profile wipe-history --profile /nix/var/nix/profiles/system  --older-than 7d

# garbage collect all unused nix store entries
	sudo nix store gc --debug

gc-all:
	nix-collect-garbage -d


fmt:
# format the nix files in this repo
	nix fmt

clean:
	rm -rf result

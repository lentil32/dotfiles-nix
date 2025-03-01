.PHONY: darwin darwin-debug update-emacs update update-all history gc fmt clean

# TODO update hostname here!
hostname := lentil32-MacBookPro

############################################################################
#
#  Darwin related commands
#
############################################################################

#  TODO Feel free to remove this target if you don't need a proxy to speed up the build process
# darwin-set-proxy:
#   sudo python3 scripts/darwin_set_proxy.py
#

# darwin: darwin-set-proxy
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

update:
	nix flake update

update-all: update update-emacs

history:
	nix profile history --profile /nix/var/nix/profiles/system

gc:
# remove all generations older than 7 days
	sudo nix profile wipe-history --profile /nix/var/nix/profiles/system  --older-than 7d

# garbage collect all unused nix store entries
	sudo nix store gc --debug


fmt:
# format the nix files in this repo
	nix fmt

clean:
	rm -rf result

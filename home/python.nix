{ pkgs, ... }:
let
  # NOTE: Python 3.14 is currently not viable in this channel for a common global stack:
  # `python314Packages.pip` and `ipython` dependency chains fail (html5lib/parso issues).
  # Keep 3.13 until nixpkgs catches up.
  #
  # Prefer a Nix-managed Python runtime + libs over Homebrew pip global installs.
  python = pkgs.python313.withPackages (
    ps: with ps; [
      beautifulsoup4
      ipython
      numpy
      pandas
      pip
      pyyaml # import name is `yaml`, package name is `pyyaml`
      requests
      virtualenv
    ]
  );
in
{
  home.packages = with pkgs; [
    python

    autoflake # remove unused imports
    black # LSP
    isort # sort imports
    pipenv
    poetry
    pyright
  ];
}

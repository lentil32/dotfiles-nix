{ pkgs, ... }:
{
  home.packages = with pkgs; [
    autoflake # remove unused imports
    libb2 # cryptography library
    black # LSP
    isort # sort imports
    pipenv
    poetry
    pyenv
    pyright
  ];
}

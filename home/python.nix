{ pkgs, ... }:
{
  home.packages = with pkgs; [
    autoflake # remove unused imports
    black # LSP
    isort # sort imports
    pipenv
    poetry
    pyenv
    pyright
  ];
}

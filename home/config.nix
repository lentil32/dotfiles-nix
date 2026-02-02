{ ... }:
{
  xdg.configFile."nix/nix.conf".text = ''
    # Managed by Home Manager. Put secrets in nix.conf.local.
    !include nix.conf.local
  '';

}

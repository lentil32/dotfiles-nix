{
  lib,
  ...
}:
let
  pnpmMinimumReleaseAgeMinutes = 3 * 24 * 60;
in
{
  xdg.configFile."nix/nix.conf".text = ''
    # Managed by Home Manager. Put secrets in nix.conf.local.
    !include nix.conf.local
  '';

  # pnpm stores global non-auth settings in npmrc-style format.
  xdg.configFile."pnpm/rc".text = lib.generators.toKeyValue { } {
    "minimum-release-age" = pnpmMinimumReleaseAgeMinutes;
  };
}

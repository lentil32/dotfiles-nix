{
  lib,
  pkgs,
  ...
}:
let
  openIsland = pkgs.stdenvNoCC.mkDerivation rec {
    pname = "open-island";
    version = "1.0.27";

    src = pkgs.fetchzip {
      url = "https://github.com/Octane0411/open-vibe-island/releases/download/v${version}/Open.Island.zip";
      hash = "sha256-FwNz1zXLKwqXveqIeya/i5vwWhlqt2fb0JoBkSFV6to=";
      stripRoot = false;
    };

    dontUnpack = true;

    installPhase = ''
      runHook preInstall

      mkdir -p "$out/Applications" "$out/bin"
      cp -R "$src/Open Island.app" "$out/Applications/"

      cat > "$out/bin/open-island" <<EOF
      #!${pkgs.runtimeShell}
      exec /usr/bin/open "$out/Applications/Open Island.app"
      EOF
      chmod +x "$out/bin/open-island"

      runHook postInstall
    '';

    meta = with lib; {
      description = "Native macOS companion for AI coding agents";
      homepage = "https://github.com/Octane0411/open-vibe-island";
      license = licenses.gpl3Only;
      mainProgram = "open-island";
      platforms = [
        "aarch64-darwin"
        "x86_64-darwin"
      ];
      sourceProvenance = [ sourceTypes.binaryNativeCode ];
    };
  };
in
{
  environment.systemPackages = [ openIsland ];
}

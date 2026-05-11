{
  lib,
  pkgs,
  ...
}:
let
  hunk = pkgs.stdenvNoCC.mkDerivation rec {
    pname = "hunk";
    version = "0.0.9";

    src = pkgs.fetchurl {
      url = "https://github.com/smolcars/hunk/releases/download/v${version}/Hunk-${version}-macos-arm64.app.tar.gz";
      hash = "sha256-tPEqfe4PJB+VHUknbj//gNpO2LTyV1pNsk86dS41ogk=";
    };

    dontUnpack = true;

    installPhase = ''
      runHook preInstall

      mkdir -p "$out/Applications" "$out/bin"
      tar -xzf "$src" -C "$out/Applications"

      cat > "$out/bin/hunk" <<EOF
      #!${pkgs.runtimeShell}
      exec /usr/bin/open "$out/Applications/Hunk.app"
      EOF
      chmod +x "$out/bin/hunk"

      runHook postInstall
    '';

    doInstallCheck = true;
    installCheckPhase = ''
      test -d "$out/Applications/Hunk.app"
      test -x "$out/bin/hunk"
    '';

    meta = with lib; {
      description = "Fast Git diff viewer and Codex orchestrator";
      homepage = "https://github.com/smolcars/hunk";
      license = licenses.gpl3Plus;
      mainProgram = "hunk";
      platforms = [ "aarch64-darwin" ];
      sourceProvenance = [ sourceTypes.binaryNativeCode ];
    };
  };

  # Full Xcode is expected on the host for Apple-provided toolchains such as
  # metal/metallib; keep the usual Unix build essentials explicit here.
  buildEssentials = with pkgs; [
    gnumake
    pkg-config
    cmake
    ninja
    libiconv
    autoconf
    automake
    libtool
    m4
  ];
in
{
  environment.systemPackages =
    with pkgs;
    [
      zsh
      git
      vim
      man-pages
      man-pages-posix
    ]
    ++ buildEssentials
    ++ [ hunk ];

  environment.variables = {
    EDITOR = "vim";
  };
}

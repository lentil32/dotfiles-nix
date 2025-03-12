final: prev: {
  aider-chat =
    let
      unstable = prev.pkgs-unstable;
      version = "0.76.2";
    in
    unstable.aider-chat.overridePythonAttrs (oldAttrs: {
      version = version;
      src = unstable.fetchFromGitHub {
        owner = "Aider-AI";
        repo = "aider";
        tag = "v${version}";
        hash = "sha256-5pmzqlFQEAACAqF12FGTHkyJjpnpuGUe0Y0cpQ0z2Bg=";
      };

      dependencies =
        oldAttrs.dependencies
        ++ (with unstable.python312.pkgs; [
          boto3
          playwright
        ]);
    });
}

final: prev: {
  aider-chat =
    let
      unstable = prev.pkgs-unstable;
      version = "0.80.1";
    in
    unstable.aider-chat.overridePythonAttrs (oldAttrs: {
      version = version;
      src = unstable.fetchFromGitHub {
        owner = "Aider-AI";
        repo = "aider";
        tag = "v${version}";
        hash = "sha256-THJW3ZORXaRTeYE6Gmtu7Efi7F0VvU2wT7d/hQjhMzU=";
      };

      dependencies =
        oldAttrs.dependencies
        ++ (with unstable.python312.pkgs; [
          boto3
          playwright
        ]);
    });
}

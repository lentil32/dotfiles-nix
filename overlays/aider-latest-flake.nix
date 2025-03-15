final: prev: {
  aider-chat =
    let
      unstable = prev.pkgs-unstable;
      version = "0.75.2";
    in
    unstable.aider-chat.overridePythonAttrs (oldAttrs: {
      version = version;
      src = unstable.fetchFromGitHub {
        owner = "Aider-AI";
        repo = "aider";
        tag = "v${version}";
        hash = "sha256-+XpvAnxsv6TbsJwTAgNdJtZxxoPXQ9cxRVUaFZCnS8w=";
      };

      dependencies =
        oldAttrs.dependencies
        ++ (with unstable.python312.pkgs; [
          boto3
          playwright
        ]);
    });
}

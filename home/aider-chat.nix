{
  config,
  pkgs,
  lib,
  ...
}:

{
  home.packages = with pkgs; [
    uv
    python312
  ];

  home.activation.setupAiderChat = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    ${pkgs.uv}/bin/uv tool install --force --python ${pkgs.python312}/bin/python3.12 aider-chat@latest --with boto3 --with playwright
    ${config.home.homeDirectory}/.local/share/uv/tools/aider-chat/bin/python -m playwright install --with-deps chromium
  '';
}

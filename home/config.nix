{ pkgs, ... }:
{
  home.file = {
    ".claude/settings.json".text = ''
      {
        "permissions": {
          "allow": [
            "Bash(ls:*)",
            "Bash(find:*)",
            "Bash(rg:*)",
            "Bash(git add:*)",
            "Read(node_modules/**)",
            "Bash(nr test:*)",
            "Bash(nr lint:*)",
            "Bash(nr format:*)"
          ],
          "deny": []
        },
        "env": {
          "DISABLE_TELEMETRY": "1",
          "DISABLE_BUG_COMMAND": "1"
        },
        "preferredNotifChannel": "terminal_bell",
        "includeCoAuthoredBy": "false"
      }
    '';
  };
}

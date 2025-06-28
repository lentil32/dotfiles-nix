{ pkgs, ... }:
{
  home.file = {
    ".claude/settings.json".text = ''
      {
        "permissions": {
          "allow": [
            "Bash(find:*)",
            "Bash(git add:*)",
            "Bash(git fetch:*)"
            "Bash(git show:*)",
            "Bash(git worktree:*)",
            "Bash(grep:*)",
            "Bash(ls:*)",
            "Bash(mkdir:*)",
            "Bash(nr format:*)",
            "Bash(nr lint:*)",
            "Bash(nr test:*)",
            "Bash(rg:*)",
            "Read(node_modules/**)"
            "mcp__context7__get-library-docs",
            "mcp__context7__resolve-library-id",
          ],
          "deny": []
        },
        "env": {
          "DISABLE_TELEMETRY": "1",
          "DISABLE_BUG_COMMAND": "1"
        },
        "model": "opus",
        "preferredNotifChannel": "terminal_bell"
      }
    '';
  };
}

{ pkgs, pkgs-unstable, ... }:
let
  meh = "ctrl-alt-cmd";
  hyper = "ctrl-alt-cmd-shift";
  terminalAppId = "com.github.wez.wezterm";
  terminalAppName = "WezTerm";
  workspaces = {
    term = "1-TERM";
    code = "2-CODE";
    web = "3-WEB_";
    comms = "4-COMM";
    docs = "5-DOCS";
    side = "6-SIDE";
    misc = "7-MISC";
    notes = "8-NOTE";
    media = "9-MEDI";
    social = "0-SOCI";
  };
in
{
  services.aerospace = {
    enable = true;
    package = pkgs-unstable.aerospace;
    settings = {
      # You can use it to add commands that run after login to macOS user session.
      # 'start-at-login' needs to be 'true' for 'after-login-command' to work
      # Available commands: https://nikitabobko.github.io/AeroSpace/commands
      after-login-command = [ ];

      # You can use it to add commands that run after AeroSpace startup.
      # 'after-startup-command' is run after 'after-login-command'
      # Available commands : https://nikitabobko.github.io/AeroSpace/commands
      after-startup-command = [ ];

      # Start AeroSpace at login
      # Note: This is managed by nix-darwin service, not by aerospace itself
      start-at-login = false;

      # Normalizations. See: https://nikitabobko.github.io/AeroSpace/guide#normalization
      # i3 doesn't have "normalizations" feature that why we disable them here.
      # But the feature is very helpful.
      # Normalizations eliminate all sorts of weird tree configurations that don't make sense.
      # Give normalizations a chance and enable them back.
      enable-normalization-flatten-containers = false;
      enable-normalization-opposite-orientation-for-nested-containers = false;

      # See: https://nikitabobko.github.io/AeroSpace/guide#layouts
      # The 'accordion-padding' specifies the size of accordion padding
      # You can set 0 to disable the padding feature
      accordion-padding = 30;

      # Possible values: tiles|accordion
      default-root-container-layout = "accordion";

      # Possible values: horizontal|vertical|auto
      # 'auto' means: wide monitor (anything wider than high) gets horizontal orientation,
      #               tall monitor (anything higher than wide) gets vertical orientation
      default-root-container-orientation = "auto";

      # Mouse follows focus when focused monitor changes
      # Drop it from your config, if you don't like this behavior
      # See https://nikitabobko.github.io/AeroSpace/guide#on-focus-changed-callbacks
      # See https://nikitabobko.github.io/AeroSpace/commands#move-mouse
      on-focused-monitor-changed = [ "move-mouse monitor-lazy-center" ];

      # Gaps between windows (inner-*) and between monitor edges (outer-*).
      # Possible values:
      # - Constant:     gaps.outer.top = 8
      # - Per monitor:  gaps.outer.top = [{ monitor.main = 16 }, { monitor."some-pattern" = 32 }, 24]
      #                 In this example, 24 is a default value when there is no match.
      #                 Monitor pattern is the same as for 'workspace-to-monitor-force-assignment'.
      #                 See: https://nikitabobko.github.io/AeroSpace/guide#assign-workspaces-to-monitors
      gaps = {
        inner.horizontal = 8;
        inner.vertical = 8;
        outer.left = 8;
        outer.bottom = 8;
        outer.top = 8;
        outer.right = 8;
      };

      workspace-to-monitor-force-assignment = {
        "${workspaces.code}" = "main";
        "${workspaces.side}" = "secondary";
      };

      on-window-detected = [
        {
          "if".app-id = "com.anthropic.claudefordesktop";
          run = "move-node-to-workspace ${workspaces.web}";
        }
        {
          "if".app-id = "com.openai.chat";
          run = "move-node-to-workspace ${workspaces.web}";
        }
        {
          "if".app-id = "company.thebrowser.Browser";
          run = "move-node-to-workspace ${workspaces.web}";
        }
        {
          "if".app-id = "app.zen-browser.zen";
          run = "move-node-to-workspace ${workspaces.web}";
        }
        {
          "if".app-id = "com.linear";
          run = "move-node-to-workspace ${workspaces.comms}";
        }
        {
          "if".app-id = "com.tinyspeck.slackmacgap";
          run = "move-node-to-workspace ${workspaces.comms}";
        }
        {
          "if".app-id = "com.neovide.neovide";
          run = "move-node-to-workspace ${workspaces.code}";
        }
        {
          "if".app-id = "com.spotify.client";
          run = "move-node-to-workspace ${workspaces.media}";
        }
        {
          "if".app-id = "com.github.th-ch.youtube-music";
          run = "move-node-to-workspace ${workspaces.media}";
        }
        {
          "if".app-id = "com.vanejung.elpy";
          run = "move-node-to-workspace ${workspaces.media}";
        }
        {
          "if".app-id = "com.hnc.Discord";
          run = "move-node-to-workspace ${workspaces.social}";
        }
        {
          "if".app-id = terminalAppId;
          run = "layout tiling";
        }
        {
          "if".app-id = "com.apple.finder";
          run = "layout floating";
        }
        {
          "if".app-id = "in.sinew.Enpass-Desktop";
          run = "layout floating";
        }
      ];

      mode.main.binding = {
        # All possible keys:
        # - Letters.        a, b, c, ..., z
        # - Numbers.        0, 1, 2, ..., 9
        # - Keypad numbers. keypad0, keypad1, keypad2, ..., keypad9
        # - F-keys.         f1, f2, ..., f20
        # - Special keys.   minus, equal, period, comma, slash, backslash, quote, semicolon, backtick,
        #                   leftSquareBracket, rightSquareBracket, space, enter, esc, backspace, tab
        # - Keypad special. keypadClear, keypadDecimalMark, keypadDivide, keypadEnter, keypadEqual,
        #                   keypadMinus, keypadMultiply, keypadPlus
        # - Arrows.         left, down, up, right

        # All possible modifiers: cmd, alt, ctrl, shift

        # All possible commands: https://nikitabobko.github.io/AeroSpace/commands

        "${meh}-enter" = "exec-and-forget open -a ${terminalAppName}";

        "${meh}-h" = "focus left";
        "${meh}-j" = "focus down";
        "${meh}-k" = "focus up";
        "${meh}-l" = "focus right";

        "${hyper}-h" = "move left";
        "${hyper}-j" = "move down";
        "${hyper}-k" = "move up";
        "${hyper}-l" = "move right";

        # Consider using 'join-with' command as a 'split' replacement if you want to enable normalizations
        "${hyper}-v" = "split horizontal";
        "${meh}-v" = "split vertical";

        "${meh}-f" = "fullscreen";

        "${meh}-s" = "layout v_accordion"; # 'layout stacking' in i3
        "${meh}-w" = "layout h_accordion"; # 'layout tabbed' in i3
        "${meh}-e" = "layout tiles horizontal vertical"; # 'layout toggle split' in i3

        "${hyper}-space" = "layout floating tiling"; # 'floating toggle' in i3

        # Not supported, because this command is redundant in AeroSpace mental model.
        # See: https://nikitabobko.github.io/AeroSpace/guide#floating-windows
        # ${meh}-space = 'focus toggle_tiling_floating'

        # `focus parent`/`focus child` not supported (won't implement)
        # ${meh}-a = 'focus parent'

        "${meh}-1" = "workspace ${workspaces.term}";
        "${meh}-2" = "workspace ${workspaces.code}";
        "${meh}-3" = "workspace ${workspaces.web}";
        "${meh}-4" = "workspace ${workspaces.comms}";
        "${meh}-5" = "workspace ${workspaces.docs}";
        "${meh}-6" = "workspace ${workspaces.side}";
        "${meh}-7" = "workspace ${workspaces.misc}";
        "${meh}-8" = "workspace ${workspaces.notes}";
        "${meh}-9" = "workspace ${workspaces.media}";
        "${meh}-0" = "workspace ${workspaces.social}";

        "${hyper}-1" = "move-node-to-workspace ${workspaces.term}";
        "${hyper}-2" = "move-node-to-workspace ${workspaces.code}";
        "${hyper}-3" = "move-node-to-workspace ${workspaces.web}";
        "${hyper}-4" = "move-node-to-workspace ${workspaces.comms}";
        "${hyper}-5" = "move-node-to-workspace ${workspaces.docs}";
        "${hyper}-6" = "move-node-to-workspace ${workspaces.side}";
        "${hyper}-7" = "move-node-to-workspace ${workspaces.misc}";
        "${hyper}-8" = "move-node-to-workspace ${workspaces.notes}";
        "${hyper}-9" = "move-node-to-workspace ${workspaces.media}";
        "${hyper}-0" = "move-node-to-workspace ${workspaces.social}";

        "${hyper}-c" = "reload-config";

        "${meh}-r" = "mode resize";
        "${hyper}-p" = "mode programs";
      };

      mode.resize.binding = {
        h = "resize width -50";
        j = "resize height +50";
        k = "resize height -50";
        l = "resize width +50";

        enter = "mode main";
        esc = "mode main";
      };

      # open programs
      mode.programs.binding = {
        a = [
          "exec-and-forget open -na \"Arc\""
          "mode main"
        ];
        b = [
          "exec-and-forget open -na \"Zen Browser\""
          "mode main"
        ];
        c = [
          "exec-and-forget open -na \"Google Chrome\""
          "mode main"
        ];
        e = [
          "exec-and-forget ${pkgs.emacs30}/bin/emacsclient -ca \"${pkgs.emacs30}/bin/emacs\""
          "mode main"
        ];
        f = [
          "exec-and-forget open -na \"Firefox\""
          "mode main"
        ];
        shift-f = [
          "exec-and-forget open -na \"Firefox Developer Edition\""
          "mode main"
        ];
        n = [
          "exec-and-forget open -a Neovide"
          "mode main"
        ];
        p = [
          "exec-and-forget open -a Enpass"
          "mode main"
        ];
        r = [
          "exec-and-forget open -a Raycast"
          "mode main"
        ];

        enter = "mode main";
        esc = "mode main";
      };
    };
  };
}

{
  config,
  pkgs,
  ...
}:
let
  nvimProfileScript = builtins.readFile ./nvim-profile.sh;
  ntfyClientConfigPath = "${config.xdg.configHome}/ntfy/client.yml";
  ntfyTopicFilePath = "${config.xdg.stateHome}/ntfy/codex-topic";
in
{
  home.packages = with pkgs; [
    hyperfine
    jq

    # Custom helper commands
    (writeShellScriptBin "codex-notify-ntfy" ''
      #!/usr/bin/env bash
      set -euo pipefail

      payload_json="''${1:-}"
      if [[ -z "$payload_json" ]]; then
        echo "codex-notify-ntfy: missing JSON payload argument" >&2
        exit 1
      fi

      if ! printf '%s' "$payload_json" | jq empty >/dev/null 2>&1; then
        echo "codex-notify-ntfy: payload is not valid JSON" >&2
        exit 1
      fi

      event_type="$(printf '%s' "$payload_json" | jq -r '.type // empty')"

      # Feed all Codex notifications into lemonaid when available.
      # This powers terminal-pane switching for Codex sessions.
      lemonaid_bin=""
      if command -v lemonaid >/dev/null 2>&1; then
        lemonaid_bin="$(command -v lemonaid)"
      elif [[ -x "$HOME/.local/bin/lemonaid" ]]; then
        lemonaid_bin="$HOME/.local/bin/lemonaid"
      fi

      if [[ -n "$lemonaid_bin" ]]; then
        if ! "$lemonaid_bin" codex notify "$payload_json"; then
          echo "codex-notify-ntfy: lemonaid codex notify failed" >&2
        fi
      fi

      case "$event_type" in
      "" | "agent-turn-complete")
        ;;
      *)
        exit 0
        ;;
      esac

      title="$(
        printf '%s' "$payload_json" | jq -r '
          .title
          // "Codex turn complete"
        '
      )"
      message="$(
        printf '%s' "$payload_json" | jq -r '
          .message
          // .["last-assistant-message"]
          // (if (.["input-messages"]? | type) == "array" then (.["input-messages"] | join(" ")) else empty end)
          // "A Codex turn completed."
        '
      )"
      tags="robot_face,white_check_mark"
      topic_file="${ntfyTopicFilePath}"
      config_path="${ntfyClientConfigPath}"
      topic="$(tr -d '[:space:]' < "$topic_file" 2>/dev/null || true)"

      if [[ -z "$topic" ]]; then
        echo "codex-notify-ntfy: missing topic in $topic_file" >&2
        exit 1
      fi

      ntfy publish --config "$config_path" --quiet --title "$title" --tags "$tags" "$topic" "$message"
    '')
    (writeShellScriptBin ",sld" ''
      #!/usr/bin/env zsh

      # Sleep display utility for macOS
      # Usage: ,sld [-l] [-s]
      #   -l    Do not mute the system volume
      #   -s    Put the entire system to sleep instead of just the display

      # Using milliseconds to avoid floating point issues
      idleTimeThresholdMs=500
      muteVolume=true
      sleepCommand="displaysleepnow"

      while getopts "ls" opt; do
        case $opt in
        l)
          muteVolume=false
          ;;
        s)
          sleepCommand="sleepnow"
          ;;
        \?)
          echo "Invalid option: -$OPTARG" >&2
          echo "Usage: $0 [-l] [-s]"
          exit 1
          ;;
        esac
      done

      getSystemIdleTimeMs() {
        ioreg -c IOHIDSystem | awk '/HIDIdleTime/ {print int($NF/1000000)}'
      }

      if $muteVolume; then
        osascript -e "set volume with output muted"
      fi

      caffeinate -s &
      caffeinate_pid=$!

      while [[ $(getSystemIdleTimeMs) -lt $idleTimeThresholdMs ]]; do
        sleep 0.1
      done

      sleep 0.25
      pmset "$sleepCommand"
      kill "$caffeinate_pid" 2>/dev/null
    '')
    (writeShellScriptBin ",nvp" nvimProfileScript)
    (writeShellScriptBin "nvim-profile" nvimProfileScript)
  ];
}

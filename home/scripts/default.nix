{
  config,
  pkgs,
  ...
}:
let
  nvimProfileScript = builtins.readFile ./nvim-profile.sh;
in
{
  home.packages = with pkgs; [
    hyperfine
    jq

    # Custom helper commands
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

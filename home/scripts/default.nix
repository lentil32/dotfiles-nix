{ pkgs, ... }:
{
  home.packages = with pkgs; [
    # Comma-prefixed commands
    (writeShellScriptBin ",sld" ''
      #!/usr/bin/env zsh

      # Sleep display utility for macOS
      # Usage: ,sld [-l] [-s]
      #   -l    Do not mute the system volume
      #   -s    Put the entire system to sleep instead of just the display

      idleTimeThreshold=0.5
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

      getSystemIdleTime() {
        ioreg -c IOHIDSystem | awk '/HIDIdleTime/ {print int($NF/1000000000)}'
      }

      if $muteVolume; then
        osascript -e "set volume with output muted"
      fi

      caffeinate -s &
      caffeinate_pid=$!

      while (($(getSystemIdleTime) < idleTimeThreshold)); do
        sleep 0.1
      done

      sleep 0.25
      pmset "$sleepCommand"
      kill "$caffeinate_pid" 2>/dev/null
    '')
  ];
}

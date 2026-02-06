{
  config,
  pkgs,
  username,
  ...
}:
let
  xdgConfigHome = config.home-manager.users.${username}.xdg.configHome;
  xdgStateHome = config.home-manager.users.${username}.xdg.stateHome;
  serverConfigPath = "${xdgConfigHome}/ntfy/server.yml";
  clientConfigPath = "${xdgConfigHome}/ntfy/client.yml";
  topicFilePath = "${xdgStateHome}/ntfy/codex-topic";
  tailscaleBin = "/opt/homebrew/bin/tailscale";

  macosNotifier = pkgs.writeShellScript "ntfy-macos-notifier" ''
    set -euo pipefail

    notification_title="''${NTFY_TITLE:-}"
    notification_message="''${NTFY_MESSAGE:-}"

    if [[ -z "$notification_title" ]]; then
      notification_title="Codex task complete"
    fi
    if [[ -z "$notification_message" ]]; then
      notification_message="A Codex task completed."
    fi

    exec "${pkgs.terminal-notifier}/bin/terminal-notifier" \
      -title "$notification_title" \
      -message "$notification_message" \
      -sender "com.github.wez.wezterm" \
      -group "codex-ntfy"
  '';

  ntfySubscriber = pkgs.writeShellScript "ntfy-codex-subscriber" ''
    set -euo pipefail

    topic_file="${topicFilePath}"
    topic="$(tr -d '[:space:]' < "$topic_file" 2>/dev/null || true)"

    if [[ -z "$topic" ]]; then
      echo "ntfy-codex-subscriber: missing topic in ${topicFilePath}" >&2
      exit 1
    fi

    exec "${pkgs.ntfy-sh}/bin/ntfy" subscribe --config "${clientConfigPath}" "$topic" "${macosNotifier}"
  '';

  ntfyServerLauncher = pkgs.writeShellScript "ntfy-server-launcher" ''
    set -euo pipefail

    base_url="http://127.0.0.1:2586"
    dns_name="$("${tailscaleBin}" status --json 2>/dev/null | "${pkgs.jq}/bin/jq" -r '.Self.DNSName // empty' | sed 's/\.$//' || true)"
    serve_config="$("${tailscaleBin}" serve status --json 2>/dev/null | "${pkgs.jq}/bin/jq" -c 'if type == "object" then . else {} end' || echo '{}')"
    if [[ -n "$dns_name" && "$serve_config" != "{}" ]]; then
      base_url="https://$dns_name"
    fi

    exec "${pkgs.ntfy-sh}/bin/ntfy" serve \
      --config "${serverConfigPath}" \
      --base-url "$base_url" \
      --behind-proxy \
      --upstream-base-url "https://ntfy.sh"
  '';

  ntfyTailscaleServe = pkgs.writeShellScript "ntfy-tailscale-serve" ''
    set -euo pipefail

    restart_ntfy_server() {
      local uid server_agent
      uid="$(id -u)"
      server_agent="gui/$uid/org.nixos.ntfy-server"
      if launchctl print "$server_agent" >/dev/null 2>&1; then
        launchctl kickstart -k "$server_agent" >/dev/null 2>&1 || true
      fi
    }

    status_json="$("${tailscaleBin}" status --json 2>/dev/null || true)"
    if [[ -z "$status_json" ]]; then
      exit 0
    fi

    backend_state="$(printf '%s' "$status_json" | "${pkgs.jq}/bin/jq" -r '.BackendState // empty')"
    if [[ "$backend_state" != "Running" ]]; then
      exit 0
    fi

    dns_name="$(printf '%s' "$status_json" | "${pkgs.jq}/bin/jq" -r '.Self.DNSName // empty' | sed 's/\.$//')"
    if [[ -z "$dns_name" ]]; then
      exit 0
    fi

    set +e
    serve_output="$("${pkgs.coreutils}/bin/timeout" 8 "${tailscaleBin}" serve --yes --bg --https=443 http://127.0.0.1:2586 2>&1)"
    serve_exit=$?
    set -e

    if [[ $serve_exit -eq 124 ]]; then
      echo "ntfy-tailscale-serve: timed out waiting for tailscale serve; keeping local-only mode." >&2
      exit 0
    fi

    if [[ $serve_exit -ne 0 ]]; then
      if printf '%s' "$serve_output" | grep -qi "Serve is not enabled on your tailnet"; then
        echo "ntfy-tailscale-serve: Serve is disabled for this tailnet; keeping local-only mode." >&2
        exit 0
      fi
      echo "ntfy-tailscale-serve: failed to configure tailscale serve: $serve_output" >&2
      exit $serve_exit
    fi

    serve_after="$("${tailscaleBin}" serve status --json 2>/dev/null | "${pkgs.jq}/bin/jq" -c 'if type == "object" then . else {} end' || echo '{}')"
    if [[ "$serve_after" != "{}" ]] && ! pgrep -f "ntfy serve .*--base-url https://$dns_name" >/dev/null 2>&1; then
      restart_ntfy_server
    fi

    exit 0
  '';
in
{
  launchd.user.agents.ntfy-server = {
    serviceConfig = {
      Label = "org.nixos.ntfy-server";
      ProgramArguments = [ "${ntfyServerLauncher}" ];
      RunAtLoad = true;
      KeepAlive = true;
      StandardOutPath = "/tmp/ntfy-server.log";
      StandardErrorPath = "/tmp/ntfy-server.err";
    };
  };

  launchd.user.agents.ntfy-codex-notifier = {
    serviceConfig = {
      Label = "org.nixos.ntfy-codex-notifier";
      ProgramArguments = [ "${ntfySubscriber}" ];
      RunAtLoad = true;
      KeepAlive = true;
      StandardOutPath = "/tmp/ntfy-codex-notifier.log";
      StandardErrorPath = "/tmp/ntfy-codex-notifier.err";
    };
  };

  launchd.user.agents.ntfy-tailscale-serve = {
    serviceConfig = {
      Label = "org.nixos.ntfy-tailscale-serve";
      ProgramArguments = [ "${ntfyTailscaleServe}" ];
      RunAtLoad = true;
      StartInterval = 300;
      StandardOutPath = "/tmp/ntfy-tailscale-serve.log";
      StandardErrorPath = "/tmp/ntfy-tailscale-serve.err";
    };
  };
}

{
  config,
  lib,
  pkgs,
  ...
}:
{
  xdg.configFile."nix/nix.conf".text = ''
    # Managed by Home Manager. Put secrets in nix.conf.local.
    !include nix.conf.local
  '';

  xdg.configFile."ntfy/server.yml".text = ''
    listen-http: "127.0.0.1:2586"
    web-root: "disable"
    cache-file: "${config.xdg.stateHome}/ntfy/cache.db"
    auth-file: "${config.xdg.stateHome}/ntfy/auth.db"
    attachment-cache-dir: "${config.xdg.stateHome}/ntfy/attachments"
  '';

  xdg.configFile."ntfy/client.yml".text = ''
    default-host: http://127.0.0.1:2586
  '';

  home.activation.ensureNtfyStateDir = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    ntfy_state_dir="${config.xdg.stateHome}/ntfy"
    topic_file="$ntfy_state_dir/codex-topic"

    mkdir -p "$ntfy_state_dir/attachments"
    chmod 700 "$ntfy_state_dir"

    if [[ ! -s "$topic_file" ]]; then
      printf 'codex-%s\n' "$(${pkgs.openssl}/bin/openssl rand -hex 16)" > "$topic_file"
    fi

    chmod 600 "$topic_file"
  '';

  home.activation.restartNtfyAgents = lib.hm.dag.entryAfter [ "ensureNtfyStateDir" ] ''
    uid="$(id -u)"
    for label in \
      "org.nixos.ntfy-tailscale-serve" \
      "org.nixos.ntfy-server" \
      "org.nixos.ntfy-codex-notifier"
    do
      agent="gui/$uid/$label"
      if launchctl print "$agent" >/dev/null 2>&1; then
        launchctl kickstart -k "$agent" >/dev/null 2>&1 || true
      fi
    done
  '';
}

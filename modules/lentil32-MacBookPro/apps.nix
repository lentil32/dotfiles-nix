{ pkgs, ... }:
let
  # Package format: { cask = "name"; } or { formulae = "name"; }
  # Optional: { desc = "description"; cask = "name"; }
  packages = [
    # Security
    { cask = "1password"; }
    { cask = "1password-cli"; }
    { cask = "cloudflare-warp"; }
    { formulae = "cloudflared"; }

    # Browsers
    { cask = "arc"; }
    {
      desc = "Needed by leetcode.el";
      cask = "brave-browser";
    }
    { cask = "firefox"; }
    { cask = "firefox@developer-edition"; }
    { cask = "google-chrome"; }

    # Productivity
    { cask = "hammerspoon"; }
    { cask = "karabiner-elements"; }
    { cask = "linear-linear"; }
    { cask = "linearmouse"; }
    { cask = "monitorcontrol"; }
    { cask = "raycast"; }
    {
      desc = "Beautiful system monitor";
      cask = "stats";
    }

    # LLM
    { cask = "chatgpt"; }
    {
      desc = "Claude Desktop";
      cask = "claude";
    }
    { cask = "claude-code"; }
    {
      desc = "OpenAI Codex CLI";
      cask = "codex";
    }
    {
      desc = "OpenAI Codex Desktop";
      cask = "codex-app";
    }
    { formulae = "anomalyco/tap/opencode"; }
    {
      desc = "Agent terminal notifications + voice packs";
      formulae = "peonping/tap/peon-ping";
    }

    # IM & audio & video
    { cask = "discord"; }
    { cask = "microsoft-teams"; }
    { cask = "slack"; }
    { cask = "spotify"; }
    {
      desc = "Open-source YouTube Music client (pear-devs/pear)";
      cask = "pear-desktop";
    }
    { formulae = "yt-dlp"; }

    # Media
    { cask = "anki"; }
    {
      desc = "Video player";
      cask = "iina";
    }
    { cask = "ogdesign-eagle"; }
    { cask = "transmission"; }
    {
      desc = "Download tool";
      formulae = "aria2";
    }

    # Development
    { cask = "cyberduck"; }
    {
      desc = "REST client";
      cask = "insomnia";
    }
    { cask = "postman"; }
    {
      desc = "Open-source X.Org X Window System";
      cask = "xquartz";
    }
    {
      desc = "Python needs arm64 version";
      formulae = "libb2";
    }
    {
      desc = "Needed by Emacs multivterm compilation";
      formulae = "libtool";
    }
    {
      desc = "Caddy uses it";
      formulae = "nss";
    }
    {
      desc = "Slack CLI for developing Slack App";
      cask = "slack-cli";
    }
    { formulae = "sqlfmt"; }
    { formulae = "supabase/tap/supabase"; }
    { formulae = "uv"; }
    { formulae = "wget"; }

    # Fun
    { cask = "dungeon-crawl-stone-soup-tiles"; }
    { cask = "millie"; }
    {
      desc = "Minecraft";
      cask = "prismlauncher";
    }

    # Utility
    { cask = "jordanbaird-ice"; }
    { cask = "tailscale-app"; }
  ];

  isFormulae = item: item ? formulae;
  isCask = item: item ? cask;
  brews = map (item: item.formulae) (builtins.filter isFormulae packages);
  casks = map (item: item.cask) (builtins.filter isCask packages);
in
{
  ##########################################################################
  #
  #  Install all apps and packages here.
  #
  #  NOTE: Your can find all available options in:
  #    https://daiderd.com/nix-darwin/manual/index.html
  #
  ##########################################################################

  environment.systemPackages = with pkgs; [
    zsh
    git
    vim
    man-pages
    man-pages-posix
  ];
  environment.variables.EDITOR = "vim";

  # Homebrew is managed by nix-homebrew (see flake.nix)
  # Taps are declared declaratively in flake.nix
  homebrew = {
    enable = true;

    onActivation = {
      autoUpdate = true;
      upgrade = true;
      cleanup = "zap";
    };

    masApps = {
      WireGuard = 1451685025;
      Xcode = 497799835;
    };

    inherit brews casks;
  };
}

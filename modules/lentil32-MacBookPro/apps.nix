{ pkgs, ... }:
let
  # Helper: items are casks by default, use { formulae = "name"; } for brews
  # Example: "firefox" -> cask, { formulae = "wget"; } -> brew
  packages = [
    # Security
    "1password"
    "1password-cli"
    "cloudflare-warp"

    # Browsers
    "arc"
    "brave-browser" # Needed by leetcode.el
    "firefox"
    "firefox@developer-edition"
    "google-chrome"

    # Productivity
    "hammerspoon"
    "karabiner-elements"
    "linear-linear"
    "linearmouse"
    "monitorcontrol"
    "raycast"
    "stats" # beautiful system monitor

    # LLM
    "chatgpt"
    "claude"
    "codex" # GPT
    { formulae = "gemini-cli"; }
    { formulae = "sst/tap/opencode"; }

    # IM & audio & video
    "discord"
    "microsoft-teams"
    "slack"
    "spotify"
    "pear-desktop" # Open-source YouTube Music client (pear-devs/pear)
    { formulae = "yt-dlp"; }

    # Media
    "anki"
    "iina" # video player
    "ogdesign-eagle"
    "transmission"
    { formulae = "aria2"; } # download tool

    # Development
    "cyberduck"
    "ghostty"
    "insomnia" # REST client
    "postman"
    "xquartz" # Open-source version of the X.Org X Window System
    { formulae = "libb2"; } # Python needs arm64 version of this
    { formulae = "libtool"; } # Needed by Emacs multivterm compilation step
    { formulae = "nss"; } # Caddy uses it
    { formulae = "sqlfmt"; }
    { formulae = "supabase/tap/supabase"; }
    { formulae = "uv"; }
    { formulae = "wget"; }

    # Fun
    "dungeon-crawl-stone-soup-tiles"
    "millie"
    "prismlauncher" # Minecraft

    # Utility
    "jordanbaird-ice"
  ];

  isFormulae = item: builtins.isAttrs item && item ? formulae;
  brews = map (item: item.formulae) (builtins.filter isFormulae packages);
  casks = builtins.filter builtins.isString packages;
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
      autoUpdate = false;
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

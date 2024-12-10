{ pkgs, ... }:
{
  ##########################################################################
  #
  #  Install all apps and packages here.
  #
  #  NOTE: Your can find all available options in:
  #    https://daiderd.com/nix-darwin/manual/index.html
  #
  #
  ##########################################################################

  # Install packages from nix's official package repository.
  #
  # The packages installed here are available to all users, and are reproducible across machines, and are rollbackable.
  # But on macOS, it's less stable than homebrew.
  #
  # Related Discussion: https://discourse.nixos.org/t/darwin-again/29331
  environment.systemPackages = with pkgs; [
    zsh
    git
    vim
    man-pages
    man-pages-posix
  ];
  environment.variables.EDITOR = "vim";

  # TODO To make this work, homebrew need to be installed manually, see https://brew.sh
  #
  # The apps installed by homebrew are not managed by nix, and not reproducible!
  # But on macOS, homebrew has a much larger selection of apps than nixpkgs, especially for GUI apps!
  homebrew = {
    enable = true;

    onActivation = {
      # Fetch the newest stable branch of Homebrew's git repo. Set false to prevent updating emacs
      autoUpdate = false;
      upgrade = true; # Upgrade outdated casks, formulae, and App Store apps
      # 'zap': uninstalls all formulae(and related files) not listed in the generated Brewfile
      cleanup = "zap";
    };

    # Applications to install from Mac App Store using mas.
    # You need to install all these Apps manually first so that your apple account have records for them.
    # otherwise Apple Store will refuse to install them.
    # For details, see https://github.com/mas-cli/mas
    masApps = {
      Structured = 1499198946;
      WireGuard = 1451685025;
      Xcode = 497799835;
    };

    taps = [
      "homebrew/bundle"
      "homebrew/services"
      "nikitabobko/tap"
      "th-ch/youtube-music"
    ];

    # `brew install`
    brews = [
      "aria2" # download tool
      "curl" # no not install curl via nixpkgs, it's not working well on macOS!
      "docker"
      "docker-compose"
      "httpie" # http client
      "libtool" # Needed by Emacs multivterm compilation step
      "sqlfmt"
      "wget"
      "yt-dlp"
    ];

    # `brew install --cask`
    casks = [
      # Browsers
      "arc"

      # Needed by leetcode.el.
      # Reference: https://github.com/kaiwk/leetcode.el/issues/104
      "brave-browser"

      "firefox"
      "firefox@developer-edition"
      # "google-chrome"

      # Productivity
      "aerospace"
      "hammerspoon"
      "karabiner-elements"
      "monitorcontrol"
      "raycast"
      "scroll-reverser"
      "stats" # beautiful system monitor

      # "visual-studio-code"

      # IM & audio & remote desktop & meeting
      "discord"
      "microsoft-teams"
      "youtube-music" # Open-source YouTube Music client
      "spotify"

      "anki"
      "iina" # video player
      "transmission"

      # Development
      "cyberduck"
      "orbstack"
      "insomnia" # REST client
      "postman"
      "xquartz" # Open-source version of the X.Org X Window System

      # Fun
      "prismlauncher" # Minecraft
      "dungeon-crawl-stone-soup-tiles"
    ];
  };
}

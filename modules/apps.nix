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
      autoUpdate = false;
      # 'zap': uninstalls all formulae(and related files) not listed here.
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
    ];

    # `brew install`
    brews = [
      "aria2" # download tool
      "curl" # no not install curl via nixpkgs, it's not working well on macOS!
      "httpie" # http client
      "libtool" # Needed by Emacs multivterm compilation step
      "wget"

      # Signing git commits in macOS
      # Set up a GPG key for signing Git commits on MacOS (M1)
      # Reference: https://gist.github.com/phortuin/cf24b1cca3258720c71ad42977e1ba57
      "pinentry-mac"
    ];

    # `brew install --cask`
    casks = [
      # Browsers
      "arc"
      # Needed by leetcode.el.
      # Reference: https://github.com/kaiwk/leetcode.el/issues/104
      "brave-browser"
      "firefox@developer-edition"
      # "google-chrome"

      # Productivity
      "hammerspoon"
      "karabiner-elements"
      "monitorcontrol"
      "raycast"
      "scroll-reverser"
      "stats" # beautiful system monitor

      # "visual-studio-code"

      # IM & audio & remote desktop & meeting
      # "telegram"
      "discord"
      "spotify"

      "anki"
      "iina" # video player
      "transmission"

      # Development
      "cyberduck"
      "insomnia" # REST client
      "xquartz" # Open-source version of the X.Org X Window System
    ];
  };
}

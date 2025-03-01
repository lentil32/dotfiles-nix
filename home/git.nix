{ lib, ... }:
{
  # `programs.git` will generate the config file: ~/.config/git/config
  # to make git use this config file, `~/.gitconfig` should not exist!
  #
  #    https://git-scm.com/docs/git-config#Documentation/git-config.txt---global
  home.activation.removeExistingGitconfig = lib.hm.dag.entryBefore [ "checkLinkTargets" ] ''
    rm -f ~/.gitconfig
  '';

  programs.git = {
    enable = true;
    lfs.enable = true;

    userName = "lentil32";
    userEmail = "lentil32@icloud.com";

    includes = [
      {
        # use diffrent email & name for work
        path = "~/work/.gitconfig";
        condition = "gitdir:~/work/";
      }
    ];

    extraConfig = {
      init.defaultBranch = "main";
      push.autoSetupRemote = true;
      pull.rebase = true;
    };

    signing = {
      key = "C69D0D84EE437EDA60F39326ED44A29A1A3B09B1";
      signByDefault = true;
    };

    delta = {
      enable = true;
      options = {
        features = "side-by-side";
      };
    };

    aliases = {
      # common aliases
      br = "branch";
      co = "checkout";
      st = "status";
      ls = ''log --pretty=format:"%C(yellow)%h%Cred%d\\ %Creset%s%Cblue\\ [%cn]" --decorate'';
      ll = ''log --pretty=format:"%C(yellow)%h%Cred%d\\ %Creset%s%Cblue\\ [%cn]" --decorate --numstat'';
      cm = "commit -m";
      ca = "commit -am";
      dc = "diff --cached";
      amend = "commit --amend -m";

      # aliases for submodule
      update = "submodule update --init --recursive";
      foreach = "submodule foreach";
    };

    ignores = [
      # This should contains things specific to a local environment, such as IDE, OS, or editor.
      # This shouldn't contain anything created or used by project command line build tools.

      # Vim
      "tags"
      "/.vim/"
      "Session.vim"

      # JVM
      "hs_err_pid*"

      # macOS
      ".DS_Store"

      # Windows
      "Thumbs.db"

      # Linux
      "nohup.out"

      # Sandbox files
      "junk*"

      # See header comment for why these aren't ignored globally:
      # *.log

      "_note.md"
      "_note.org"
      "_notes"
    ];
  };
}

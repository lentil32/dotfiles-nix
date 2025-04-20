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

    # https://blog.gitbutler.com/how-git-core-devs-configure-git/
    extraConfig = {
      github.user = "lentil32";
      core.autocrlf = "input";
      log.date = "iso";

      # clearly makes git better
      column.ui = "auto";
      branch.sort = "-committerdate";
      tag.sort = "version:refname";
      init.defaultBranch = "main";
      diff = {
        algorithm = "histogram";
        colorMoved = "plain";
        mnemonicPrefix = true;
        renames = true;
        submodule = "log";
      };
      push = {
        default = "simple";
        autoSetupRemote = true;
        followTags = true;
      };
      fetch = {
        prune = true;
        pruneTags = true;
        all = true;
      };

      # why the hell not?
      help.autocorrect = "prompt";
      # commit.verbose = true; I use Magit, so don't need it
      rerere = {
        enabled = true;
        autoupdate = true;
      };
      rebase = {
        autoSquash = true;
        autoStash = true;
        updateRefs = true;
      };

      # a matter of taste (uncomment if you dare)
      core = {
        fsmonitor = true;
        untrackedCache = true;
      };
      merge.conflictstyle = "zdiff3";
      pull.rebase = true;
    };

    signing = {
      key = "C69D0D84EE437EDA60F39326ED44A29A1A3B09B1";
      signByDefault = true;
    };

    aliases = {
      # common aliases
      br = "branch";
      ci = "commit";
      co = "checkout";
      st = "status -s";
      dump = "cat-file -p";
      type = "cat-file -t";
      hist = "log --pretty=format:\"%h %ad | %s%d [%an]\" --graph --date=short";
      lol = "log --graph --oneline --decorate --color --all";
      wow = "log --graph --oneline --decorate --color --all --simplify-by-decoration";
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

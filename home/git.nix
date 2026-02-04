{ lib, config, ... }:
let
  homeDir = config.home.homeDirectory;
  maintenanceSecretFile = ../secrets/git-maintenance.yaml;
  hasMaintenanceSecret = builtins.pathExists maintenanceSecretFile;
  maintenanceInclude = {
    path = "${homeDir}/.config/git/maintenance.inc";
  };
in
{
  # `programs.git` will generate the config file: ~/.config/git/config
  # to make git use this config file, `~/.gitconfig` should not exist!
  #
  #    https://git-scm.com/docs/git-config#Documentation/git-config.txt---global
  home.activation.removeExistingGitconfig = lib.hm.dag.entryBefore [ "checkLinkTargets" ] ''
    rm -f "${homeDir}/.gitconfig"
  '';

  programs.delta = {
    enable = true;
    enableGitIntegration = true;
    options = {
      features = "side-by-side";
    };
  };

  programs.git = {
    enable = true;
    lfs.enable = true;

    includes = [
      {
        # use diffrent email & name for work
        path = "${homeDir}/work/.gitconfig";
        condition = "gitdir:${homeDir}/work/";
      }
    ]
    ++ lib.optional hasMaintenanceSecret maintenanceInclude;

    signing = {
      key = "C69D0D84EE437EDA60F39326ED44A29A1A3B09B1";
      signByDefault = true;
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

      ".direnv"

      "_note.md"
      "_note.org"
      "_notes"
    ];

    settings = {
      user = {
        name = "lentil32";
        email = "lentil32@icloud.com";
      };

      alias = {
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
        negotiationAlgorithm = "skipping";
        parallel = 8;
        writeCommitGraph = true;
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
      pack.threads = 0;
    };
  };
}

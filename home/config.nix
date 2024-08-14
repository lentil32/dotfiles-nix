{ pkgs, config, ... }:
{
  home = {
    file.".gnupg/gpg-agent.conf".text = ''
      allow-emacs-pinentry
      allow-loopback-pinentry
    '';

    file.".gnupg/gpg.conf".text = ''
      #
      # This is an implementation of the Riseup OpenPGP Best Practices
      # https://help.riseup.net/en/security/message-security/openpgp/best-practices
      #


      #-----------------------------
      # default key
      #-----------------------------

      # The default key to sign with. If this option is not used, the default key is
      # the first key found in the secret keyring

      # TODO Put your own PGP key
      default-key 0xC69D0D84EE437EDA60F39326ED44A29A1A3B09B1


      #-----------------------------
      # behavior
      #-----------------------------

      # Disable inclusion of the version string in ASCII armored output
      no-emit-version

      # Disable comment string in clear text signatures and ASCII armored messages
      no-comments

      # Display long key IDs
      keyid-format 0xlong

      # List all keys (or the specified ones) along with their fingerprints
      with-fingerprint

      # Display the calculated validity of user IDs during key listings
      list-options show-uid-validity
      verify-options show-uid-validity

      # Try to use the GnuPG-Agent. With this option, GnuPG first tries to connect to
      # the agent before it asks for a passphrase.
      use-agent


      #-----------------------------
      # keyserver
      #-----------------------------

      # This is the server that --recv-keys, --send-keys, and --search-keys will
      # communicate with to receive keys from, send keys to, and search for keys on
      keyserver hkps://hkps.pool.sks-keyservers.net

      # Provide a certificate store to override the system default
      # Get this from https://sks-keyservers.net/sks-keyservers.netCA.pem
      #keyserver-options ca-cert-file=/usr/local/etc/ssl/certs/hkps.pool.sks-keyservers.net.pem

      # Set the proxy to use for HTTP and HKP keyservers - default to the standard
      # local Tor socks proxy
      # It is encouraged to use Tor for improved anonymity. Preferrably use either a
      # dedicated SOCKSPort for GnuPG and/or enable IsolateDestPort and
      # IsolateDestAddr
      #keyserver-options http-proxy=socks5-hostname://127.0.0.1:9050

      # Don't leak DNS, see https://trac.torproject.org/projects/tor/ticket/2846
      #keyserver-options no-try-dns-srv

      # When using --refresh-keys, if the key in question has a preferred keyserver
      # URL, then disable use of that preferred keyserver to refresh the key from
      keyserver-options no-honor-keyserver-url

      # When searching for a key with --search-keys, include keys that are marked on
      # the keyserver as revoked
      keyserver-options include-revoked


      #-----------------------------
      # algorithm and ciphers
      #-----------------------------

      # list of personal digest preferences. When multiple digests are supported by
      # all recipients, choose the strongest one
      personal-cipher-preferences AES256 AES192 AES CAST5

      # list of personal digest preferences. When multiple ciphers are supported by
      # all recipients, choose the strongest one
      personal-digest-preferences SHA512 SHA384 SHA256 SHA224

      # message digest algorithm used when signing a key
      cert-digest-algo SHA512

      # This preference list is used for new keys and becomes the default for
      # "setpref" in the edit menu
      default-preference-list SHA512 SHA384 SHA256 SHA224 AES256 AES192 AES CAST5 ZLIB BZIP2 ZIP Uncompressed
    '';

    file."${config.xdg.configHome}/aerospace/aerospace.toml".text = ''
      # Reference: https://github.com/i3/i3/blob/next/etc/config

      # You can use it to add commands that run after login to macOS user session.
      # 'start-at-login' needs to be 'true' for 'after-login-command' to work
      # Available commands: https://nikitabobko.github.io/AeroSpace/commands
      after-login-command = []

      # You can use it to add commands that run after AeroSpace startup.
      # 'after-startup-command' is run after 'after-login-command'
      # Available commands : https://nikitabobko.github.io/AeroSpace/commands
      after-startup-command = []

      # Start AeroSpace at login
      start-at-login = true

      # Normalizations. See: https://nikitabobko.github.io/AeroSpace/guide#normalization
      # i3 doesn't have "normalizations" feature that why we disable them here.
      # But the feature is very helpful.
      # Normalizations eliminate all sorts of weird tree configurations that don't make sense.
      # Give normalizations a chance and enable them back.
      enable-normalization-flatten-containers = false
      enable-normalization-opposite-orientation-for-nested-containers = false

      # See: https://nikitabobko.github.io/AeroSpace/guide#layouts
      # The 'accordion-padding' specifies the size of accordion padding
      # You can set 0 to disable the padding feature
      accordion-padding = 30

      # Possible values: tiles|accordion
      default-root-container-layout = 'tiles'

      # Possible values: horizontal|vertical|auto
      # 'auto' means: wide monitor (anything wider than high) gets horizontal orientation,
      #               tall monitor (anything higher than wide) gets vertical orientation
      default-root-container-orientation = 'auto'

      # Mouse follows focus when focused monitor changes
      # Drop it from your config, if you don't like this behavior
      # See https://nikitabobko.github.io/AeroSpace/guide#on-focus-changed-callbacks
      # See https://nikitabobko.github.io/AeroSpace/commands#move-mouse
      on-focused-monitor-changed = ['move-mouse monitor-lazy-center']

      # Gaps between windows (inner-*) and between monitor edges (outer-*).
      # Possible values:
      # - Constant:     gaps.outer.top = 8
      # - Per monitor:  gaps.outer.top = [{ monitor.main = 16 }, { monitor."some-pattern" = 32 }, 24]
      #                 In this example, 24 is a default value when there is no match.
      #                 Monitor pattern is the same as for 'workspace-to-monitor-force-assignment'.
      #                 See: https://nikitabobko.github.io/AeroSpace/guide#assign-workspaces-to-monitors
      [gaps]
      inner.horizontal = 0
      inner.vertical =   0
      outer.left =       0
      outer.bottom =     65
      outer.top =        0
      outer.right =      0

      [workspace-to-monitor-force-assignment]
      2 = 'main'
      6 = 'secondary'

      # [[on-window-detected]]
      # if.app-id = 'org.gnu.Emacs'
      # run = 'move-node-to-workspace 2' # mnemonics I - IDE

      # [[on-window-detected]]
      # if.app-id = 'company.thebrowser.Browser'
      # run = 'move-node-to-workspace 3' # mnemonics W - Web browser

      [[on-window-detected]]
      if.app-id = 'com.spotify.client'
      run = 'move-node-to-workspace 4' # mnemonics M - Media

      [[on-window-detected]]
      if.app-id = 'com.hnc.Discord'
      run = 'move-node-to-workspace 5' # mnemonics S - Social Network

      [mode.main.binding]
      # All possible keys:
      # - Letters.        a, b, c, ..., z
      # - Numbers.        0, 1, 2, ..., 9
      # - Keypad numbers. keypad0, keypad1, keypad2, ..., keypad9
      # - F-keys.         f1, f2, ..., f20
      # - Special keys.   minus, equal, period, comma, slash, backslash, quote, semicolon, backtick,
      #                   leftSquareBracket, rightSquareBracket, space, enter, esc, backspace, tab
      # - Keypad special. keypadClear, keypadDecimalMark, keypadDivide, keypadEnter, keypadEqual,
      #                   keypadMinus, keypadMultiply, keypadPlus
      # - Arrows.         left, down, up, right

      # All possible modifiers: cmd, alt, ctrl, shift

      # All possible commands: https://nikitabobko.github.io/AeroSpace/commands

      ctrl-alt-cmd-enter = 'exec-and-forget open -na Alacritty'

      ctrl-alt-cmd-h = 'focus left'
      ctrl-alt-cmd-j = 'focus down'
      ctrl-alt-cmd-k = 'focus up'
      ctrl-alt-cmd-l = 'focus right'

      ctrl-alt-cmd-shift-h = 'move left'
      ctrl-alt-cmd-shift-j = 'move down'
      ctrl-alt-cmd-shift-k = 'move up'
      ctrl-alt-cmd-shift-l = 'move right'

      # Consider using 'join-with' command as a 'split' replacement if you want to enable normalizations
      ctrl-alt-cmd-shift-v = 'split horizontal'
      ctrl-alt-cmd-v = 'split vertical'

      ctrl-alt-cmd-f = 'fullscreen'

      ctrl-alt-cmd-s = 'layout v_accordion' # 'layout stacking' in i3
      ctrl-alt-cmd-w = 'layout h_accordion' # 'layout tabbed' in i3
      ctrl-alt-cmd-e = 'layout tiles horizontal vertical' # 'layout toggle split' in i3

      ctrl-alt-cmd-shift-space = 'layout floating tiling' # 'floating toggle' in i3

      # Not supported, because this command is redundant in AeroSpace mental model.
      # See: https://nikitabobko.github.io/AeroSpace/guide#floating-windows
      # ctrl-alt-cmd-space = 'focus toggle_tiling_floating'

      # `focus parent`/`focus child` are not yet supported, and it's not clear whether they
      # should be supported at all https://github.com/nikitabobko/AeroSpace/issues/5
      # ctrl-alt-cmd-a = 'focus parent'

      ctrl-alt-cmd-1 = 'workspace 1'
      ctrl-alt-cmd-2 = 'workspace 2'
      ctrl-alt-cmd-3 = 'workspace 3'
      ctrl-alt-cmd-4 = 'workspace 4'
      ctrl-alt-cmd-5 = 'workspace 5'
      ctrl-alt-cmd-6 = 'workspace 6'
      ctrl-alt-cmd-7 = 'workspace 7'
      ctrl-alt-cmd-8 = 'workspace 8'
      ctrl-alt-cmd-9 = 'workspace 9'
      ctrl-alt-cmd-0 = 'workspace 10'

      ctrl-alt-cmd-shift-1 = 'move-node-to-workspace 1'
      ctrl-alt-cmd-shift-2 = 'move-node-to-workspace 2'
      ctrl-alt-cmd-shift-3 = 'move-node-to-workspace 3'
      ctrl-alt-cmd-shift-4 = 'move-node-to-workspace 4'
      ctrl-alt-cmd-shift-5 = 'move-node-to-workspace 5'
      ctrl-alt-cmd-shift-6 = 'move-node-to-workspace 6'
      ctrl-alt-cmd-shift-7 = 'move-node-to-workspace 7'
      ctrl-alt-cmd-shift-8 = 'move-node-to-workspace 8'
      ctrl-alt-cmd-shift-9 = 'move-node-to-workspace 9'
      ctrl-alt-cmd-shift-0 = 'move-node-to-workspace 10'

      ctrl-alt-cmd-shift-c = 'reload-config'

      ctrl-alt-cmd-r = 'mode resize'
      ctrl-alt-cmd-shift-p = 'mode programs'

      [mode.resize.binding]
      h = 'resize width -50'
      j = 'resize height +50'
      k = 'resize height -50'
      l = 'resize width +50'

      enter = 'mode main'
      esc = 'mode main'

      # open programs
      [mode.programs.binding]
      a = [ 'exec-and-forget open -a Arc',                                       'mode main' ]
      c = [ 'exec-and-forget open -na "Google Chrome"',                          'mode main' ]
      e = [ 'exec-and-forget ${pkgs.emacs}/bin/emacsclient -ca "open -a Emacs"', 'mode main' ]
      f = [ 'exec-and-forget open -na "Firefox Developer Edition"',              'mode main' ]
      p = [ 'exec-and-forget open -a Enpass',                                    'mode main' ]
      r = [ 'exec-and-forget open -a Raycast',                                   'mode main' ]

      enter = 'mode main'
      esc = 'mode main'
    '';
  };
}

{
  config,
  lib,
  pkgs,
  ...
}:
let
  gnupgDir = "${config.home.homeDirectory}/.gnupg";
  signingKey = "C69D0D84EE437EDA60F39326ED44A29A1A3B09B1";
in
{
  home.file = {
    ".gnupg/gpg-agent.conf".text = ''
      default-cache-ttl 18000
      max-cache-ttl 18000
      pinentry-program ${pkgs.pinentry_mac}/bin/pinentry-mac
      enable-ssh-support
    '';
    ".gnupg/gpg.conf".text = ''
      #
      # GnuPG defaults for this dotfiles repo.
      #

      #-----------------------------
      # default key
      #-----------------------------

      # Use the personal signing key for this machine/user.
      default-key 0x${signingKey}

      #-----------------------------
      # behavior
      #-----------------------------

      # Do not include extra metadata in armored output.
      no-emit-version
      no-comments

      # Show long key IDs and fingerprints in listings.
      keyid-format 0xlong
      with-fingerprint

      # Show UID validity during listings and verification output.
      list-options show-uid-validity
      verify-options show-uid-validity

      #-----------------------------
      # key discovery
      #-----------------------------

      # Prefer local keys and WKD before falling back to a public keyserver.
      auto-key-locate local,wkd,keyserver

      # Use a modern HKPS keyserver instead of the legacy SKS pool.
      keyserver hkps://keys.openpgp.org

      # Ignore per-key preferred keyserver URLs when refreshing.
      keyserver-options no-honor-keyserver-url

      #-----------------------------
      # algorithms and ciphers
      #-----------------------------

      # Prefer strong personal defaults when multiple algorithms are available.
      personal-cipher-preferences AES256 AES192 AES CAST5
      personal-digest-preferences SHA512 SHA384 SHA256 SHA224
      cert-digest-algo SHA512
      default-preference-list SHA512 SHA384 SHA256 SHA224 AES256 AES192 AES CAST5 ZLIB BZIP2 ZIP Uncompressed
    '';
  };

  home.activation.ensureGnupgPermissions = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    # GnuPG is strict about directory modes. Home Manager manages the files,
    # so normalize the parent directories after links are written.
    mkdir -p "${gnupgDir}"
    chmod 700 "${gnupgDir}"

    if [[ -d "${gnupgDir}/private-keys-v1.d" ]]; then
      chmod 700 "${gnupgDir}/private-keys-v1.d"
    fi

    if [[ -d "${gnupgDir}/openpgp-revocs.d" ]]; then
      chmod 700 "${gnupgDir}/openpgp-revocs.d"
    fi
  '';
}

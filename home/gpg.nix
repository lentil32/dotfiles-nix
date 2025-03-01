{ pkgs, ... }:
{
  home.file = {
    ".gnupg/gpg-agent.conf".text = ''
      max-cache-ttl 18000
      default-cache-ttl 18000
      pinentry-program ${pkgs.pinentry_mac}/bin/pinentry-mac
      enable-ssh-support
    '';
    ".gnupg/gpg.conf".text = ''
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
  };
}

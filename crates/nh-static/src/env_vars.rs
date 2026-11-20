use crate::Documented;

/// Names of every environment variable nh reads, sets or forwards.
pub struct EnvVars;

/// Declares documented environment variables as associated constants on
/// [`EnvVars`] and collects them, in declaration order, into
/// [`EnvVars::DOCUMENTED`].
macro_rules! documented {
  ($( $(#[doc = $doc:literal])+ $name:ident; )*) => {
    impl EnvVars {
      $(
        $(#[doc = $doc])+
        pub const $name: &'static str = stringify!($name);
      )*

      /// Environment variables documented in nh's man page, in order.
      pub const DOCUMENTED: &'static [Documented] = &[
        $( Documented { name: Self::$name, doc: concat!($($doc, "\n"),+) } ),*
      ];
    }
  };
}

documented! {
  /// When set to `1`, `true`, or `yes`, asks for confirmation before applying
  /// supported operations. `0`, `false`, or `no` disable it; other values are
  /// rejected. Equivalent to `--ask`.
  NH_ASK;

  /// Preferred path or reference to a directory containing your `flake.nix`,
  /// used by nh when running flake-based commands. Historically `FLAKE` was
  /// used; nh migrates `FLAKE` into `NH_FLAKE` if present and none of the
  /// command-specific `NH_*_FLAKE` variables are set.
  NH_FLAKE;

  /// Flake reference for `nh os` commands. Takes precedence over `NH_FLAKE`.
  NH_OS_FLAKE;

  /// Flake reference for `nh home` commands. Takes precedence over
  /// `NH_FLAKE`.
  NH_HOME_FLAKE;

  /// Flake reference for `nh darwin` commands. Takes precedence over
  /// `NH_FLAKE`.
  NH_DARWIN_FLAKE;

  /// Path to a directory or file containing a Nix expression. Chosen after
  /// the command-specific `NH_*_FLAKE` variables, but before `NH_FLAKE`.
  NH_FILE;

  /// Attribute path into the expression given by `NH_FILE`, for example
  /// `nixosConfigurations.<hostname>`. If both are set, nh evaluates
  /// `$NH_FILE#$NH_ATTRP`.
  NH_ATTRP;

  /// When set to any value, skips startup checks such as Nix version and
  /// experimental feature validation. Useful for generating completions or
  /// running in constrained build environments.
  NH_NO_CHECKS;

  /// Controls whether rom shows build progress. Unset, rom runs only when
  /// stdout is a terminal. Set to `1`, `true`, `yes`, or `on` to force it on,
  /// or `0`, `false`, `no`, or `off` to force it off. `--no-rom` takes
  /// precedence. `NH_NOM` is a deprecated alias that will be removed in 5.0.
  NH_ROM;

  /// Path to a file that records the Nix log stream of builds shown by rom,
  /// so it can be replayed through rom.
  NH_ROM_LOG;

  /// Sets the tracing filter for nh, in `tracing_subscriber` env filter
  /// format, for example `nh=trace`.
  NH_LOG;

  /// Privilege elevation strategy: `auto`, `none`, `passwordless`,
  /// `empty-password`, or `program:<path>`. Use `empty-password` for remote
  /// sudo setups where PAM authenticates out-of-band, such as `pam_rssh` with
  /// a security key, and expects an empty password line on stdin. Equivalent
  /// to `--elevation-strategy`.
  NH_ELEVATION_STRATEGY;

  /// Path to a program used as `SUDO_ASKPASS` when nh self-elevates with
  /// `sudo`. If set and `sudo` is used, nh passes `-A` to it.
  NH_SUDO_ASKPASS;

  /// Extra arguments inserted into the `sudo` invocation when nh elevates
  /// privileges. Accepts the same shell-quoted format as `NIX_SUDOOPTS`, and
  /// takes precedence over it.
  NH_SUDOOPTS;

  /// Controls whether environment variables marked for preservation are
  /// passed to elevated commands. Set to `0` to disable preservation or `1` to
  /// force it. Unset, preservation is enabled.
  NH_PRESERVE_ENV;

  /// SSH options for remote operations. Accepts the same format as
  /// `NIX_SSHOPTS`, and takes precedence over it.
  NH_SSHOPTS;

  /// Don't refuse to run when invoked as root. Equivalent to
  /// `--bypass-root-check`.
  NH_BYPASS_ROOT_CHECK;

  /// Skip pre-activation system validation checks. Equivalent to
  /// `--no-validate`.
  NH_NO_VALIDATE;

  /// Run `switch-to-configuration` directly instead of in a transient
  /// systemd unit. Equivalent to `--no-systemd-run`.
  NH_NO_SYSTEMD_RUN;

  /// For `nh os switch`, activation runs before the new generation is added
  /// to the bootloader, and a failed activation aborts the switch. When set,
  /// nh continues to the bootloader step regardless, so the generation is
  /// still bootable. Equivalent to `--continue-on-activation-failure`.
  NH_CONTINUE_ON_ACTIVATION_FAILURE;

  /// Deprecated: activation output is always shown. Will be removed in 5.0.
  NH_SHOW_ACTIVATION_LOGS;

  /// Overrides the path to the nh configuration file. Unset, nh uses
  /// `$XDG_CONFIG_HOME/nh/config.toml`, falling back to
  /// `~/.config/nh/config.toml`.
  NH_CONFIG;

  /// GitHub token used by `nh search prs` and `nh search issues`. Takes
  /// priority over any token file.
  GH_TOKEN;

  /// Overrides the path to the GitHub token file used by `nh search prs` and
  /// `nh search issues` when `GH_TOKEN` is unset. Unset, nh uses
  /// `$XDG_STATE_HOME/nh/github-token`, falling back to
  /// `~/.local/state/nh/github-token`.
  NH_GITHUB_TOKEN_FILE;

  /// Default Nixpkgs channel used by `nh search`, for example
  /// `nixos-unstable`. Equivalent to `--channel`.
  NH_SEARCH_CHANNEL;

  /// When set to `1`, `true`, or `yes`, `nh search` prints results as JSON.
  /// Equivalent to `--json`.
  NH_SEARCH_JSON;

  /// When set to `1`, `true`, or `yes`, `nh search` shows supported platforms
  /// for each package. Equivalent to `--platforms`.
  NH_SEARCH_PLATFORM;

  /// Target of the `nh search <query>` shorthand: `packages` (the default) or
  /// `options`. Equivalent to `--default-search`.
  NH_DEFAULT_SEARCH;

  /// Colon-separated list of SPAM database files used by
  /// `nh search offline`. Equivalent to passing `--db` once per path.
  NH_OFFLINE_DB;

  /// SSH options passed to Nix commands for remote builds. `NH_SSHOPTS` takes
  /// precedence when both are set.
  NIX_SSHOPTS;

  /// Extra arguments inserted into the `sudo` invocation, supported for
  /// nixos-rebuild compatibility. `NH_SUDOOPTS` takes precedence when both
  /// are set.
  NIX_SUDOOPTS;

  /// Nix configuration, forwarded to all Nix commands.
  NIX_CONFIG;

  /// Nix remote store configuration, forwarded to all Nix commands.
  NIX_REMOTE;

  /// TLS certificate file for Nix, forwarded to all Nix commands.
  NIX_SSL_CERT_FILE;

  /// User configuration files for Nix, forwarded to all Nix commands.
  NIX_USER_CONF_FILES;

  /// Forwarded to `switch-to-configuration`. If `1`, forces bootloader
  /// installation. Also available as `--install-bootloader`.
  NIXOS_INSTALL_BOOTLOADER;

  /// Forwarded to `switch-to-configuration` during activation. Inhibits
  /// certain NixOS service checks.
  NIXOS_NO_CHECK;
}

/// Variables nh reads or forwards that are not part of its documented
/// interface.
impl EnvVars {
  /// Legacy name for `NH_FLAKE`, migrated at startup.
  pub const FLAKE: &'static str = "FLAKE";
  /// Deprecated alias for `NH_ROM`.
  pub const NH_NOM: &'static str = "NH_NOM";
  /// Deprecated name for `NH_ELEVATION_STRATEGY`.
  pub const NH_ELEVATION_PROGRAM: &'static str = "NH_ELEVATION_PROGRAM";
  /// Nix search path, forwarded to Nix commands.
  pub const NIX_PATH: &'static str = "NIX_PATH";
  /// Backup extension read by home-manager activation.
  pub const HOME_MANAGER_BACKUP_EXT: &'static str = "HOME_MANAGER_BACKUP_EXT";
  /// Askpass program read by `sudo -A`.
  pub const SUDO_ASKPASS: &'static str = "SUDO_ASKPASS";
  /// glibc locale archive, forwarded so locales work in elevated commands.
  pub const LOCALE_ARCHIVE: &'static str = "LOCALE_ARCHIVE";
  /// SSH agent socket.
  pub const SSH_AUTH_SOCK: &'static str = "SSH_AUTH_SOCK";
  pub const HOME: &'static str = "HOME";
  pub const USER: &'static str = "USER";
  pub const PATH: &'static str = "PATH";
  pub const TERM: &'static str = "TERM";
  pub const NO_COLOR: &'static str = "NO_COLOR";
  pub const XDG_CONFIG_HOME: &'static str = "XDG_CONFIG_HOME";
  pub const XDG_DATA_HOME: &'static str = "XDG_DATA_HOME";
  pub const XDG_STATE_HOME: &'static str = "XDG_STATE_HOME";
  pub const XDG_RUNTIME_DIR: &'static str = "XDG_RUNTIME_DIR";
}

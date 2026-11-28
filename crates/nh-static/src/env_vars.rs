use crate::{Documented, Since};

/// Names of every environment variable nh reads, sets or forwards.
pub struct EnvVars;

/// Declares documented environment variables as associated constants on
/// [`EnvVars`] and collects them, in declaration order, into
/// [`EnvVars::DOCUMENTED`].
///
/// Every variable takes `#[since("x.y.z")]`, naming the release that
/// introduced it, or `#[since(next)]` if it is not released yet. Releasing
/// replaces `next` with the new version.
macro_rules! documented {
  ($( $(#[doc = $doc:literal])+ #[since($since:tt)] $name:ident; )*) => {
    impl EnvVars {
      $(
        $(#[doc = $doc])+
        #[doc = ""]
        #[doc = since_doc!($since)]
        pub const $name: &'static str = stringify!($name);
      )*

      /// Environment variables documented in nh's man page.
      pub const DOCUMENTED: &'static [Documented] = &[
        $(
          Documented {
            name:  Self::$name,
            doc:   concat!($($doc, "\n"),+),
            since: Some(since!($since)),
          }
        ),*
      ];
    }
  };
  ($($tokens:tt)*) => {
    compile_error!(
      "each documented environment variable needs doc comments, then \
       `#[since(\"x.y.z\")]` or `#[since(next)]`, then its name"
    );
  };
}

macro_rules! since {
  (next) => {
    Since::NextRelease
  };
  ($version:literal) => {
    Since::Version($version)
  };
}

macro_rules! since_doc {
  (next) => {
    "Added in the next release of nh."
  };
  ($version:literal) => {
    concat!("Added in nh ", $version, ".")
  };
}

documented! {
  /// When set to `1`, `true`, or `yes`, asks for confirmation before applying
  /// supported operations. `0`, `false`, or `no` disable it. Equivalent to
  /// `--ask`.
  #[since("4.5.0")]
  NH_ASK;

  /// Preferred path/reference to a directory containing your `flake.nix` used
  /// by NH when running flake-based commands. Historically `FLAKE` was used;
  /// NH will migrate `FLAKE` into `NH_FLAKE` if present and `NH_FLAKE` is
  /// not set.
  #[since("4.0.0")]
  NH_FLAKE;

  /// Command-specific flake reference for `os` commands. If present it takes
  /// precedence over `NH_FLAKE`.
  #[since("4.0.0")]
  NH_OS_FLAKE;

  /// Command-specific flake reference for `home` commands. If present it
  /// takes precedence over `NH_FLAKE`.
  #[since("4.0.0")]
  NH_HOME_FLAKE;

  /// Command-specific flake reference for `darwin` commands. If present it
  /// takes precedence over `NH_FLAKE`.
  #[since("4.0.0")]
  NH_DARWIN_FLAKE;

  /// Preferred path to a directory/file containing a Nix expression. Chosen
  /// after os/home/darwin-specific env vars, but before `NH_FLAKE`.
  #[since("4.0.0")]
  NH_FILE;

  /// Nix attribute path for an expression specified with `NH_FILE`. For
  /// example, `nixosConfigurations.<hostname>`. If `NH_FILE` and `NH_ATTRP`
  /// are both set, `nh os switch` effectively evaluates `$NH_FILE#$NH_ATTRP`.
  #[since("4.0.0")]
  NH_ATTRP;

  /// When set (any value), skips startup checks such as Nix version and
  /// experimental feature validation. Useful for running in constrained build
  /// environments. You can also consider this
  /// an "expert flag" that you can set for a non-zero performance benefit. It
  /// assumes you know what you are doing.
  #[since("4.1.0")]
  NH_NO_CHECKS;

  /// Forces whether rom shows build progress. Without it, rom runs only when
  /// stdout is a terminal, and stays off when stdout is a pipe or file, such
  /// as in CI or under an agent. Set to `1`, `true`, `yes`, or `on` to force
  /// rom on, or `0`, `false`, `no`, or `off` to force it off. `--no-rom` still
  /// takes precedence. `NH_NOM` is a deprecated alias that will be removed in
  /// 5.0.
  #[since("4.5.0")]
  NH_ROM;

  /// Path to a file that records the Nix log stream of builds shown by rom,
  /// so it can be replayed through rom.
  #[since("4.5.0")]
  NH_ROM_LOG;

  /// Sets the tracing/log filter for NH. This uses the same format as
  /// `tracing_subscriber` env filters (for example: `nh=trace`).
  #[since("4.0.0")]
  NH_LOG;

  /// Privilege elevation strategy. Accepted values include `auto`, `none`,
  /// `passwordless`, `empty-password`, and `program:<path>`. Use
  /// `empty-password` for remote sudo setups where PAM authenticates
  /// out-of-band, such as `pam_rssh` with a security key, and expects an
  /// empty stdin password line.
  #[since("4.3.0")]
  NH_ELEVATION_STRATEGY;

  /// Path to a program used as `SUDO_ASKPASS` when NH self-elevates with
  /// `sudo`. If set and `sudo` is used for elevation, NH will pass `-A` to
  /// `sudo` and set `SUDO_ASKPASS` accordingly.
  #[since("4.0.3")]
  NH_SUDO_ASKPASS;

  /// Extra arguments inserted into the `sudo` invocation when NH elevates
  /// privileges. Accepts the same shell-quoted format as `NIX_SUDOOPTS`,
  /// which it takes precedence over.
  #[since("4.4.0")]
  NH_SUDOOPTS;

  /// Controls whether environment variables marked for preservation are
  /// passed to elevated commands. Set to `"0"` to disable preservation, `"1"`
  /// to force preservation. If unset, preservation defaults to enabled.
  #[since("4.2.0")]
  NH_PRESERVE_ENV;

  /// SSH options for remote operations. Accepts the same format as
  /// `NIX_SSHOPTS`, which it takes precedence over.
  #[since("4.4.0")]
  NH_SSHOPTS;

  /// Don't panic if calling nh as root.
  #[since("3.5.24")]
  NH_BYPASS_ROOT_CHECK;

  /// Skip pre-activation system validation checks.
  #[since("4.3.0")]
  NH_NO_VALIDATE;

  /// Run switch-to-configuration directly instead of in a transient systemd
  /// unit.
  #[since("4.5.0")]
  NH_NO_SYSTEMD_RUN;

  /// For `nh os switch`, activation runs before the new generation is added
  /// to the bootloader. By default a failed activation aborts the switch, so
  /// the generation is not added to the bootloader. Setting this to `"1"` (or
  /// passing `--continue-on-activation-failure`) continues to the bootloader
  /// step regardless, so the generation is still bootable on next reboot.
  #[since("4.5.0")]
  NH_CONTINUE_ON_ACTIVATION_FAILURE;

  /// Show activation logs. Deprecated: activation output is always shown, and
  /// this will be removed in 5.0.
  #[since("4.3.0")]
  NH_SHOW_ACTIVATION_LOGS;

  /// GitHub token used by `nh search prs` and `nh search issues`. If set,
  /// this takes priority over any token file.
  #[since("4.4.0")]
  GH_TOKEN;

  /// Overrides the path to the GitHub token file used by `nh search prs` and
  /// `nh search issues` when `GH_TOKEN` is unset. If unset, NH uses
  /// `$XDG_STATE_HOME/nh/github-token`, falling back to
  /// `~/.local/state/nh/github-token`. Tokens entered through the interactive
  /// prompt are saved to this file with user-only permissions.
  #[since("4.4.0")]
  NH_GITHUB_TOKEN_FILE;

  /// Default Nixpkgs channel used by `nh search packages` and
  /// `nh search options` (e.g. `nixos-unstable`, `nixos-24.11`). Overridden
  /// per-invocation by `--channel`.
  #[since("4.0.0")]
  NH_SEARCH_CHANNEL;

  /// When set to a truthy value, `nh search` outputs results as raw JSON
  /// instead of the formatted display. Equivalent to `--json`.
  #[since("4.0.2")]
  NH_SEARCH_JSON;

  /// When set to a truthy value, supported platforms are shown for each
  /// package result. Equivalent to `--platforms`.
  #[since("4.0.0")]
  NH_SEARCH_PLATFORM;

  /// Controls the target of the `nh search <query>` shorthand when no
  /// subcommand is given. Accepted values: `packages` (default), `options`
  /// (uses scope `all`). Equivalent to `--default-search`.
  #[since("4.4.0")]
  NH_DEFAULT_SEARCH;

  /// Colon-separated list of paths to SPAM database files used by
  /// `nh search offline`. Each path is treated as a separate database.
  /// Equivalent to passing `--db` multiple times. Example:
  /// `NH_OFFLINE_DB=/var/cache/spam/nixpkgs.db:/var/cache/spam/hm.db`.
  #[since("4.4.0")]
  NH_OFFLINE_DB;

  /// Forwarded in all Nix commands with environment isolation. `NH_SSHOPTS`
  /// takes precedence over `NIX_SSHOPTS` when both are set.
  #[since("4.2.0")]
  NIX_SSHOPTS;

  /// Extra sudo arguments forwarded to the `sudo` invocation; supported for
  /// nixos-rebuild compatibility. `NH_SUDOOPTS` takes precedence when both
  /// are set.
  #[since("4.4.0")]
  NIX_SUDOOPTS;

  /// Forwarded in all Nix commands with environment isolation.
  #[since("4.2.0")]
  NIX_CONFIG;

  /// Forwarded in all Nix commands with environment isolation.
  #[since("4.2.0")]
  NIX_REMOTE;

  /// Forwarded in all Nix commands with environment isolation.
  #[since("4.2.0")]
  NIX_SSL_CERT_FILE;

  /// Forwarded in all Nix commands with environment isolation.
  #[since("4.2.0")]
  NIX_USER_CONF_FILES;

  /// This is a variable accepted by `switch-to-configuration`, which handles
  /// the system switching behind the scenes. If `1`,
  /// `switch-to-configuration` will call the necessary script to force
  /// installation of your bootloader. This behaviour can also be replicated
  /// by passing `--install-bootloader` to `nh os switch` and `nh os boot`
  /// commands.
  #[since("4.2.0")]
  NIXOS_INSTALL_BOOTLOADER;

  /// Forwarded to `switch-to-configuration` during activation. If `1`, skips
  /// its pre-switch checks.
  #[since("4.4.0")]
  NIXOS_NO_CHECK;
}

/// Variables nh reads or forwards that are not part of its documented
/// interface.
impl EnvVars {
  /// Legacy name for `NH_FLAKE`, migrated at startup.
  pub const FLAKE: &'static str = "FLAKE";
  /// Deprecated alias for `NH_ROM`.
  pub const NH_NOM: &'static str = "NH_NOM";
  /// Path to nh's configuration file. Not documented until nh reads the file.
  pub const NH_CONFIG: &'static str = "NH_CONFIG";
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

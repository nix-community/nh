use nh_remote::RemoteHost;
use pound::{Parse, ValueEnum};

use crate::{
  common::{CommonRebuildArgs, Diff},
  installable::InstallableArgs,
};

#[derive(Parse, Debug)]
/// `NixOS` functionality
///
/// Implements functionality mostly around but not exclusive to nixos-rebuild
pub struct OsArgs {
  #[pound(subcommand)]
  pub subcommand: OsSubcommand,
}

#[derive(Debug, Parse)]
pub enum OsSubcommand {
  /// Build and activate the new configuration, and make it the boot default
  Switch {
    #[pound(flatten)]
    args: OsRebuildActivateArgs,
  },

  /// Build the new configuration and make it the boot default
  Boot {
    #[pound(flatten)]
    args: OsRebuildActivateArgs,
  },

  /// Build and activate the new configuration
  Test {
    #[pound(flatten)]
    args: OsRebuildActivateArgs,
  },

  /// Build and preview activation without applying it
  DryActivate {
    #[pound(flatten)]
    args: OsDryActivateArgs,
  },

  /// Build the new configuration
  Build {
    #[pound(flatten)]
    args: OsBuildArgs,
  },

  /// Load system in a repl
  Repl {
    #[pound(flatten)]
    args: OsReplArgs,
  },

  /// List available generations from profile path
  Info {
    #[pound(flatten)]
    args: OsGenerationsArgs,
  },

  /// Rollback to a previous generation
  Rollback {
    #[pound(flatten)]
    args: OsRollbackArgs,
  },

  /// Build a `NixOS` VM image
  BuildVm {
    #[pound(flatten)]
    args: OsBuildVmArgs,
  },

  /// Build a `NixOS` disk-image variant
  BuildImage {
    #[pound(flatten)]
    args: OsBuildImageArgs,
  },
}

#[derive(Debug, Parse)]
pub struct OsBuildImageArgs {
  #[pound(flatten)]
  pub common: OsRebuildArgs,

  /// Image variant
  ///
  /// If not provided, the command will fallback to
  /// `config.system.build.isoImage`.
  #[pound(long)]
  pub image_variant: Option<String>,
}

#[derive(Debug, Parse)]
pub struct OsBuildVmArgs {
  #[pound(flatten)]
  pub common: OsRebuildArgs,

  /// Build with bootloader. Bootloader is bypassed by default.
  #[pound(long, short = 'B')]
  pub with_bootloader: bool,

  /// Run the VM immediately after building
  #[pound(long, short = 'r')]
  pub run: bool,
}

#[derive(Debug, Parse)]
pub struct OsRebuildArgs {
  #[pound(flatten)]
  pub common: CommonRebuildArgs,

  #[pound(flatten)]
  pub update_args: crate::common::UpdateArgs,

  /// When using a flake installable, select this hostname from
  /// nixosConfigurations
  ///
  /// When unspecified, defaults to the local hostname for local
  /// deployments, and hostname of the target machine for remote
  /// deployments (see --target-host).
  #[pound(long, short = 'H', global)]
  pub hostname: Option<String>,

  /// Explicitly select some specialisation
  #[pound(long, short)]
  pub specialisation: Option<String>,

  /// Ignore specialisations
  #[pound(long, short = 'S')]
  pub no_specialisation: bool,

  /// Extra arguments passed to nix build
  #[pound(last)]
  pub extra_args: Vec<String>,

  /// Don't panic if calling nh as root
  #[pound(short = 'R', long, env = "NH_BYPASS_ROOT_CHECK")]
  pub bypass_root_check: bool,

  /// Deploy the built configuration to a different host over SSH
  ///
  /// Diffing against the target host requires dix >= 3.0.0 on it, unless
  /// both closures are already in the local store.
  #[pound(long, parse = str::parse::<RemoteHost>)]
  pub target_host: Option<RemoteHost>,

  /// Build the configuration on a different host over SSH
  #[pound(long, parse = str::parse::<RemoteHost>)]
  pub build_host: Option<RemoteHost>,

  /// Skip pre-activation system validation checks
  #[pound(long, env = "NH_NO_VALIDATE")]
  pub no_validate: bool,
}

#[derive(Debug, Parse)]
pub struct OsDryActivateArgs {
  #[pound(flatten)]
  pub rebuild: OsRebuildArgs,

  /// Run switch-to-configuration directly instead of in a transient systemd
  /// unit
  #[pound(long, env = "NH_NO_SYSTEMD_RUN")]
  pub no_systemd_run: bool,
}

#[derive(Debug, Parse)]
pub struct OsBuildArgs {
  #[pound(flatten)]
  pub rebuild: OsRebuildArgs,

  /// Don't create an out-link
  #[pound(long, group = "link")]
  pub no_link: bool,
}

#[derive(Debug, Parse)]
pub struct OsRebuildActivateArgs {
  #[pound(flatten)]
  pub rebuild: OsRebuildArgs,

  /// Install bootloader for switch and boot commands
  #[pound(long)]
  pub install_bootloader: bool,

  /// Show activation logs
  #[deprecated(
    since = "4.5.0",
    note = "activation output is always shown, and this flag will be removed \
            in 5.0"
  )]
  #[pound(long, env = "NH_SHOW_ACTIVATION_LOGS")]
  pub show_activation_logs: bool,

  /// Add the generation to the bootloader even if activation fails
  ///
  /// During `switch`, activation runs before the bootloader step; by default a
  /// failed activation aborts before it. This continues regardless.
  #[pound(long, env = "NH_CONTINUE_ON_ACTIVATION_FAILURE")]
  pub continue_on_activation_failure: bool,

  /// Run switch-to-configuration directly instead of in a transient systemd
  /// unit
  #[pound(long, env = "NH_NO_SYSTEMD_RUN")]
  pub no_systemd_run: bool,
}

#[derive(Debug, Parse)]
pub struct OsRollbackArgs {
  /// Only print actions, without performing them
  #[pound(long, short = 'n')]
  pub dry: bool,

  /// Ask for confirmation
  #[pound(long, short, env = "NH_ASK")]
  pub ask: bool,

  /// Explicitly select some specialisation
  #[pound(long, short)]
  pub specialisation: Option<String>,

  /// Ignore specialisations
  #[pound(long, short = 'S')]
  pub no_specialisation: bool,

  /// Rollback to a specific generation number (defaults to previous
  /// generation)
  #[pound(long, short)]
  pub to: Option<u64>,

  /// Don't panic if calling nh as root
  #[pound(short = 'R', long, env = "NH_BYPASS_ROOT_CHECK")]
  pub bypass_root_check: bool,

  /// Whether to display a package diff
  #[pound(long, short, default = "auto")]
  pub diff: Diff,

  /// Run switch-to-configuration directly instead of in a transient systemd
  /// unit
  #[pound(long, env = "NH_NO_SYSTEMD_RUN")]
  pub no_systemd_run: bool,
}

#[derive(Debug, Parse)]
pub struct OsReplArgs {
  #[pound(flatten)]
  pub installable: InstallableArgs,

  /// When using a flake installable, select this hostname from
  /// nixosConfigurations
  #[pound(long, short = 'H', global)]
  pub hostname: Option<String>,
}

#[derive(Debug, Parse)]
pub struct OsGenerationsArgs {
  /// Path to Nix' profiles directory
  #[pound(long, short = 'P', default = "/nix/var/nix/profiles/system")]
  pub profile: Option<String>,

  /// Comma-delimited list of field(s) to display
  #[pound(long, delimiter = ',')]
  pub fields: Option<Vec<Field>>,

  /// Print generation status and metadata as JSON
  #[pound(long, conflicts_with = "fields")]
  pub json: bool,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum Field {
  /// Generation Id
  Id,

  /// Build Date
  Date,

  /// Nixos Version
  Nver,

  /// Kernel Version
  Kernel,

  /// Configuration Revision
  #[pound(name = "confRev")]
  Confrev,

  /// Specialisations
  Spec,

  /// Closure Size
  Size,
}

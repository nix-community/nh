use std::{env, path::PathBuf};

use clap::{Args, Subcommand, ValueEnum};
use nh_core::checks::{
  FeatureRequirements,
  FlakeFeatures,
  LegacyFeatures,
  OsReplFeatures,
};
use nh_installable::Installable;
use nh_passthrough::NixBuildPassthroughArgs;
use nh_remote::RemoteHost;

use crate::{
  // Result,
  //   checks::{
  //     DarwinReplFeatures,
  //     FeatureRequirements,
  //     FlakeFeatures,
  //     HomeReplFeatures,
  //     LegacyFeatures,
  //     NoFeatures,
  //     OsReplFeatures,
  //   },
  //   commands::ElevationStrategy,
  generations::Field,
  //   remote::RemoteHost,
};

#[derive(Args, Debug)]
#[clap(verbatim_doc_comment)]
/// `NixOS` functionality
///
/// Implements functionality mostly around but not exclusive to nixos-rebuild
pub struct OsArgs {
  #[command(subcommand)]
  pub subcommand: OsSubcommand,
}

impl OsArgs {
  #[must_use]
  pub fn get_feature_requirements(&self) -> Box<dyn FeatureRequirements> {
    match &self.subcommand {
      OsSubcommand::Repl(args) => {
        let is_flake = args.uses_flakes();
        Box::new(OsReplFeatures { is_flake })
      },
      OsSubcommand::Switch(args)
      | OsSubcommand::Boot(args)
      | OsSubcommand::Test(args) => {
        if args.rebuild.uses_flakes() {
          Box::new(FlakeFeatures)
        } else {
          Box::new(LegacyFeatures)
        }
      },
      OsSubcommand::Build(args) => {
        if args.uses_flakes() {
          Box::new(FlakeFeatures)
        } else {
          Box::new(LegacyFeatures)
        }
      },
      OsSubcommand::BuildVm(args) => {
        if args.common.uses_flakes() {
          Box::new(FlakeFeatures)
        } else {
          Box::new(LegacyFeatures)
        }
      },
      OsSubcommand::Info(_) | OsSubcommand::Rollback(_) => {
        Box::new(LegacyFeatures)
      },

      OsSubcommand::BuildImage(args) => {
        if args.common.uses_flakes() {
          Box::new(FlakeFeatures)
        } else {
          Box::new(LegacyFeatures)
        }
      },
    }
  }
}

#[derive(Debug, Subcommand)]
pub enum OsSubcommand {
  /// Build and activate the new configuration, and make it the boot default
  Switch(OsRebuildActivateArgs),

  /// Build the new configuration and make it the boot default
  Boot(OsRebuildActivateArgs),

  /// Build and activate the new configuration
  Test(OsRebuildActivateArgs),

  /// Build the new configuration
  Build(OsRebuildArgs),

  /// Load system in a repl
  Repl(OsReplArgs),

  /// List available generations from profile path
  Info(OsGenerationsArgs),

  /// Rollback to a previous generation
  Rollback(OsRollbackArgs),

  /// Build a `NixOS` VM image
  BuildVm(OsBuildVmArgs),

  /// Build a `NixOS` disk-image variant
  BuildImage(OsBuildImageArgs),
}

#[derive(Debug, Args)]
pub struct OsBuildImageArgs {
  #[command(flatten)]
  pub common: OsRebuildArgs,

  /// Image variant
  #[arg(long)]
  pub image_variant: String,
}

#[derive(Debug, Args)]
pub struct OsBuildVmArgs {
  #[command(flatten)]
  pub common: OsRebuildArgs,

  /// Build with bootloader. Bootloader is bypassed by default.
  #[arg(long, short = 'B')]
  pub with_bootloader: bool,

  /// Run the VM immediately after building
  #[arg(long, short = 'r')]
  pub run: bool,
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct OsRebuildArgs {
  #[command(flatten)]
  pub common: CommonRebuildArgs,

  #[command(flatten)]
  pub update_args: nh_util::update::UpdateArgs,

  /// When using a flake installable, select this hostname from
  /// nixosConfigurations
  ///
  /// When unspecified, defaults to the local hostname for local
  /// deployments, and hostname of the target machine for remote
  /// deployments (see --target-host).
  #[arg(long, short = 'H', global = true)]
  pub hostname: Option<String>,

  /// Explicitly select some specialisation
  #[arg(long, short)]
  pub specialisation: Option<String>,

  /// Ignore specialisations
  #[arg(long, short = 'S')]
  pub no_specialisation: bool,

  /// Install bootloader for switch and boot commands
  #[arg(long)]
  pub install_bootloader: bool,

  /// Extra arguments passed to nix build
  #[arg(last = true)]
  pub extra_args: Vec<String>,

  /// Don't panic if calling nh as root
  #[arg(short = 'R', long, env = "NH_BYPASS_ROOT_CHECK")]
  pub bypass_root_check: bool,

  /// Deploy the built configuration to a different host over SSH
  #[arg(long)]
  pub target_host: Option<RemoteHost>,

  /// Build the configuration on a different host over SSH
  #[arg(long)]
  pub build_host: Option<RemoteHost>,

  /// Skip pre-activation system validation checks
  #[arg(long, env = "NH_NO_VALIDATE")]
  pub no_validate: bool,
}

#[derive(Debug, Args)]
pub struct OsRebuildActivateArgs {
  #[command(flatten)]
  pub rebuild: OsRebuildArgs,

  /// Show activation logs
  #[arg(long, env = "NH_SHOW_ACTIVATION_LOGS")]
  pub show_activation_logs: bool,
}

impl OsRebuildArgs {
  #[must_use]
  pub fn uses_flakes(&self) -> bool {
    // Check environment variables first
    if env::var("NH_OS_FLAKE").is_ok_and(|v| !v.is_empty()) {
      return true;
    }

    // Check installable type
    matches!(self.common.installable, Installable::Flake { .. })
  }
}

#[derive(ValueEnum, Clone, Default, Debug)]
pub enum DiffType {
  /// Display package diff only if the of the
  /// current and the deployed configuration matches
  #[default]
  Auto,
  /// Always display package diff
  Always,
  /// Never display package diff
  Never,
}

#[derive(Debug, Args)]
pub struct OsRollbackArgs {
  /// Only print actions, without performing them
  #[arg(long, short = 'n')]
  pub dry: bool,

  /// Ask for confirmation
  #[arg(long, short)]
  pub ask: bool,

  /// Explicitly select some specialisation
  #[arg(long, short)]
  pub specialisation: Option<String>,

  /// Ignore specialisations
  #[arg(long, short = 'S')]
  pub no_specialisation: bool,

  /// Rollback to a specific generation number (defaults to previous
  /// generation)
  #[arg(long, short)]
  pub to: Option<u64>,

  /// Don't panic if calling nh as root
  #[arg(short = 'R', long, env = "NH_BYPASS_ROOT_CHECK")]
  pub bypass_root_check: bool,

  /// Whether to display a package diff
  #[arg(long, short, value_enum, default_value_t = DiffType::Auto)]
  pub diff: DiffType,
}

#[derive(Debug, Args)]
pub struct CommonRebuildArgs {
  /// Only print actions, without performing them
  #[arg(long, short = 'n')]
  pub dry: bool,

  /// Ask for confirmation
  #[arg(long, short)]
  pub ask: bool,

  #[command(flatten)]
  pub installable: Installable,

  /// Don't use nix-output-monitor for the build process
  #[arg(long)]
  pub no_nom: bool,

  /// Path to save the result link, defaults to using a temporary directory
  #[arg(long, short)]
  pub out_link: Option<PathBuf>,

  /// Whether to display a package diff
  #[arg(long, short, value_enum, default_value_t = DiffType::Auto)]
  pub diff: DiffType,

  #[command(flatten)]
  pub passthrough: NixBuildPassthroughArgs,
}

#[derive(Debug, Args)]
pub struct OsReplArgs {
  #[command(flatten)]
  pub installable: Installable,

  /// When using a flake installable, select this hostname from
  /// nixosConfigurations
  #[arg(long, short = 'H', global = true)]
  pub hostname: Option<String>,
}

impl OsReplArgs {
  #[must_use]
  pub fn uses_flakes(&self) -> bool {
    // Check environment variables first
    if env::var("NH_OS_FLAKE").is_ok() {
      return true;
    }

    // Check installable type
    matches!(self.installable, Installable::Flake { .. })
  }
}

#[derive(Debug, Args)]
pub struct OsGenerationsArgs {
  /// Path to Nix' profiles directory
  #[arg(long, short = 'P', default_value = "/nix/var/nix/profiles/system")]
  pub profile: Option<String>,

  /// Comma-delimited list of field(s) to display
  #[arg(long, value_delimiter = ',')]
  pub fields: Option<Vec<Field>>,
}

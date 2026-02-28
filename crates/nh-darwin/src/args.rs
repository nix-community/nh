use std::env;

use clap::{Args, Subcommand};
use nh_core::{
  args::CommonRebuildArgs,
  checks::{
    DarwinReplFeatures,
    FeatureRequirements,
    FlakeFeatures,
    LegacyFeatures,
  },
};
use nh_installable::Installable;
use nh_remote::RemoteHost;
use nh_util::update::UpdateArgs;

/// Nix-darwin functionality
///
/// Implements functionality mostly around but not exclusive to darwin-rebuild
#[derive(Debug, Args)]
pub struct DarwinArgs {
  #[command(subcommand)]
  pub subcommand: DarwinSubcommand,
}

impl DarwinArgs {
  #[must_use]
  pub fn get_feature_requirements(&self) -> Box<dyn FeatureRequirements> {
    match &self.subcommand {
      DarwinSubcommand::Repl(args) => {
        let is_flake = args.uses_flakes();
        Box::new(DarwinReplFeatures { is_flake })
      },
      DarwinSubcommand::Switch(args) | DarwinSubcommand::Build(args) => {
        if args.uses_flakes() {
          Box::new(FlakeFeatures)
        } else {
          Box::new(LegacyFeatures)
        }
      },
    }
  }
}

#[derive(Debug, Subcommand)]
pub enum DarwinSubcommand {
  /// Build and activate a nix-darwin configuration
  Switch(DarwinRebuildArgs),
  /// Build a nix-darwin configuration
  Build(DarwinRebuildArgs),
  /// Load a nix-darwin configuration in a Nix REPL
  Repl(DarwinReplArgs),
}

#[derive(Debug, Args)]
pub struct DarwinRebuildArgs {
  #[command(flatten)]
  pub common: CommonRebuildArgs,

  #[command(flatten)]
  pub update_args: UpdateArgs,

  /// When using a flake installable, select this hostname from
  /// darwinConfigurations
  #[arg(long, short = 'H', global = true)]
  pub hostname: Option<String>,

  /// Extra arguments passed to nix build
  #[arg(last = true)]
  pub extra_args: Vec<String>,

  /// Don't panic if calling nh as root
  #[arg(short = 'R', long, env = "NH_BYPASS_ROOT_CHECK")]
  pub bypass_root_check: bool,

  /// Show activation logs
  #[arg(long, env = "NH_SHOW_ACTIVATION_LOGS")]
  pub show_activation_logs: bool,

  /// Build the configuration on a different host over SSH
  #[arg(long)]
  pub build_host: Option<RemoteHost>,
}

impl DarwinRebuildArgs {
  #[must_use]
  pub fn uses_flakes(&self) -> bool {
    // Check environment variables first
    if env::var("NH_DARWIN_FLAKE").is_ok_and(|v| !v.is_empty()) {
      return true;
    }

    // Check installable type
    matches!(self.common.installable, Installable::Flake { .. })
  }
}

#[derive(Debug, Args)]
pub struct DarwinReplArgs {
  #[command(flatten)]
  pub installable: Installable,

  /// When using a flake installable, select this hostname from
  /// darwinConfigurations
  #[arg(long, short = 'H', global = true)]
  pub hostname: Option<String>,
}

impl DarwinReplArgs {
  #[must_use]
  pub fn uses_flakes(&self) -> bool {
    // Check environment variables first
    if env::var("NH_DARWIN_FLAKE").is_ok_and(|v| !v.is_empty()) {
      return true;
    }

    // Check installable type
    matches!(self.installable, Installable::Flake { .. })
  }
}

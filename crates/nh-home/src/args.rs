use std::env;

use clap::{Args, Subcommand};
use nh_core::{
  args::CommonRebuildArgs,
  checks::{
    FeatureRequirements,
    FlakeFeatures,
    HomeReplFeatures,
    LegacyFeatures,
  },
};
use nh_installable::Installable;
use nh_remote::RemoteHost;

#[derive(Debug, Subcommand)]
pub enum HomeSubcommand {
  /// Build and activate a home-manager configuration
  Switch(HomeRebuildArgs),

  /// Build a home-manager configuration
  Build(HomeRebuildArgs),

  /// Load a home-manager configuration in a Nix REPL
  Repl(HomeReplArgs),
}

#[derive(Debug, Args)]
/// Home-manager functionality
pub struct HomeArgs {
  #[command(subcommand)]
  pub subcommand: HomeSubcommand,
}

impl HomeArgs {
  #[must_use]
  pub fn get_feature_requirements(&self) -> Box<dyn FeatureRequirements> {
    match &self.subcommand {
      HomeSubcommand::Repl(args) => {
        let is_flake = args.uses_flakes();
        Box::new(HomeReplFeatures { is_flake })
      },
      HomeSubcommand::Switch(args) | HomeSubcommand::Build(args) => {
        if args.uses_flakes() {
          Box::new(FlakeFeatures)
        } else {
          Box::new(LegacyFeatures)
        }
      },
    }
  }
}

#[derive(Debug, Args)]
pub struct HomeRebuildArgs {
  #[command(flatten)]
  pub common: CommonRebuildArgs,

  #[command(flatten)]
  pub update_args: nh_util::update::UpdateArgs,

  /// Name of the flake homeConfigurations attribute, like username@hostname
  ///
  /// If unspecified, will try <username>@<hostname> and <username>
  #[arg(long, short)]
  pub configuration: Option<String>,

  /// Explicitly select some specialisation
  #[arg(long, short)]
  pub specialisation: Option<String>,

  /// Ignore specialisations
  #[arg(long, short = 'S')]
  pub no_specialisation: bool,

  /// Extra arguments passed to nix build
  #[arg(last = true)]
  pub extra_args: Vec<String>,

  /// Move existing files by backing up with this file extension
  #[arg(long, short = 'b')]
  pub backup_extension: Option<String>,

  /// Show activation logs
  #[arg(long, env = "NH_SHOW_ACTIVATION_LOGS")]
  pub show_activation_logs: bool,

  /// Build the configuration on a different host over SSH
  #[arg(long)]
  pub build_host: Option<RemoteHost>,
}

impl HomeRebuildArgs {
  #[must_use]
  pub fn uses_flakes(&self) -> bool {
    // Check environment variables first
    if env::var("NH_HOME_FLAKE").is_ok_and(|v| !v.is_empty()) {
      return true;
    }

    // Check installable type
    matches!(self.common.installable, Installable::Flake { .. })
  }
}

#[derive(Debug, Args)]
pub struct HomeReplArgs {
  #[command(flatten)]
  pub installable: Installable,

  /// Name of the flake homeConfigurations attribute, like username@hostname
  ///
  /// If unspecified, will try <username>@<hostname> and <username>
  #[arg(long, short)]
  pub configuration: Option<String>,

  /// Extra arguments passed to nix repl
  #[arg(last = true)]
  pub extra_args: Vec<String>,
}

impl HomeReplArgs {
  #[must_use]
  pub fn uses_flakes(&self) -> bool {
    // Check environment variables first
    if env::var("NH_HOME_FLAKE").is_ok_and(|v| !v.is_empty()) {
      return true;
    }

    // Check installable type
    matches!(self.installable, Installable::Flake { .. })
  }
}

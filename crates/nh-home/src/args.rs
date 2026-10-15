use nh_core::{
  args::CommonRebuildArgs,
  checks::{
    FeatureRequirements,
    FlakeFeatures,
    HomeReplFeatures,
    LegacyFeatures,
  },
};
use nh_installable::{CommandContext, InstallableArgs};
use nh_remote::RemoteHost;
use pound::Parse;

#[derive(Debug, Parse)]
pub enum HomeSubcommand {
  /// Build and activate a home-manager configuration
  Switch {
    #[pound(flatten)]
    args: HomeRebuildArgs,
  },

  /// Build a home-manager configuration
  Build {
    #[pound(flatten)]
    args: HomeRebuildArgs,
  },

  /// Load a home-manager configuration in a Nix REPL
  Repl {
    #[pound(flatten)]
    args: HomeReplArgs,
  },
}

#[derive(Debug, Parse)]
/// Home-manager functionality
pub struct HomeArgs {
  #[pound(subcommand)]
  pub subcommand: HomeSubcommand,
}

impl HomeArgs {
  #[must_use]
  pub fn get_feature_requirements(&self) -> Box<dyn FeatureRequirements> {
    match &self.subcommand {
      HomeSubcommand::Repl { args } => {
        let is_flake = args.uses_flakes();
        Box::new(HomeReplFeatures { is_flake })
      },
      HomeSubcommand::Switch { args } | HomeSubcommand::Build { args } => {
        if args.uses_flakes() {
          Box::new(FlakeFeatures)
        } else {
          Box::new(LegacyFeatures)
        }
      },
    }
  }
}

#[derive(Debug, Parse)]
pub struct HomeRebuildArgs {
  #[pound(flatten)]
  pub common: CommonRebuildArgs,

  #[pound(flatten)]
  pub update_args: nh_core::update::UpdateArgs,

  /// Name of the flake homeConfigurations attribute, like username@hostname
  ///
  /// If unspecified, will try <username>@<hostname> and <username>
  #[pound(long, short)]
  pub configuration: Option<String>,

  /// Explicitly select some specialisation
  #[pound(long, short)]
  pub specialisation: Option<String>,

  /// Ignore specialisations
  #[pound(long, short = 'S')]
  pub no_specialisation: bool,

  /// Extra arguments passed to nix build
  #[pound(last)]
  pub extra_args: Vec<String>,

  /// Move existing files by backing up with this file extension
  #[pound(long, short = 'b')]
  pub backup_extension: Option<String>,

  /// Show activation logs
  #[pound(long, env = "NH_SHOW_ACTIVATION_LOGS")]
  pub show_activation_logs: bool,

  /// Build the configuration on a different host over SSH
  #[pound(long, parse = str::parse::<RemoteHost>)]
  pub build_host: Option<RemoteHost>,
}

impl HomeRebuildArgs {
  #[must_use]
  pub fn uses_flakes(&self) -> bool {
    self.common.installable.uses_flakes(CommandContext::Home)
  }
}

#[derive(Debug, Parse)]
pub struct HomeReplArgs {
  #[pound(flatten)]
  pub installable: InstallableArgs,

  /// Name of the flake homeConfigurations attribute, like username@hostname
  ///
  /// If unspecified, will try <username>@<hostname> and <username>
  #[pound(long, short)]
  pub configuration: Option<String>,

  /// Extra arguments passed to nix repl
  #[pound(last)]
  pub extra_args: Vec<String>,
}

impl HomeReplArgs {
  #[must_use]
  pub fn uses_flakes(&self) -> bool {
    self.installable.uses_flakes(CommandContext::Home)
  }
}

use nh_core::{
  args::CommonRebuildArgs,
  checks::{
    DarwinReplFeatures,
    FeatureRequirements,
    FlakeFeatures,
    LegacyFeatures,
  },
  update::UpdateArgs,
};
use nh_installable::{CommandContext, InstallableArgs};
use nh_remote::RemoteHost;
use pound::Parse;

/// Nix-darwin functionality
///
/// Implements functionality mostly around but not exclusive to darwin-rebuild
#[derive(Debug, Parse)]
pub struct DarwinArgs {
  #[pound(subcommand)]
  pub subcommand: DarwinSubcommand,
}

impl DarwinArgs {
  #[must_use]
  pub fn get_feature_requirements(&self) -> Box<dyn FeatureRequirements> {
    match &self.subcommand {
      DarwinSubcommand::Repl { args } => {
        let is_flake = args.uses_flakes();
        Box::new(DarwinReplFeatures { is_flake })
      },
      DarwinSubcommand::Switch { args } | DarwinSubcommand::Build { args } => {
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
pub enum DarwinSubcommand {
  /// Build and activate a nix-darwin configuration
  Switch {
    #[pound(flatten)]
    args: DarwinRebuildArgs,
  },
  /// Build a nix-darwin configuration
  Build {
    #[pound(flatten)]
    args: DarwinRebuildArgs,
  },
  /// Load a nix-darwin configuration in a Nix REPL
  Repl {
    #[pound(flatten)]
    args: DarwinReplArgs,
  },
}

#[derive(Debug, Parse)]
pub struct DarwinRebuildArgs {
  #[pound(flatten)]
  pub common: CommonRebuildArgs,

  #[pound(flatten)]
  pub update_args: UpdateArgs,

  /// When using a flake installable, select this hostname from
  /// darwinConfigurations
  #[pound(long, short = 'H', global)]
  pub hostname: Option<String>,

  /// Extra arguments passed to nix build
  #[pound(last)]
  pub extra_args: Vec<String>,

  /// Don't panic if calling nh as root
  #[pound(short = 'R', long, env = "NH_BYPASS_ROOT_CHECK")]
  pub bypass_root_check: bool,

  /// Show activation logs
  #[pound(long, env = "NH_SHOW_ACTIVATION_LOGS")]
  pub show_activation_logs: bool,

  /// Build the configuration on a different host over SSH
  #[pound(long, parse = str::parse::<RemoteHost>)]
  pub build_host: Option<RemoteHost>,
}

impl DarwinRebuildArgs {
  #[must_use]
  pub fn uses_flakes(&self) -> bool {
    self.common.installable.uses_flakes(CommandContext::Darwin)
  }
}

#[derive(Debug, Parse)]
pub struct DarwinReplArgs {
  #[pound(flatten)]
  pub installable: InstallableArgs,

  /// When using a flake installable, select this hostname from
  /// darwinConfigurations
  #[pound(long, short = 'H', global)]
  pub hostname: Option<String>,
}

impl DarwinReplArgs {
  #[must_use]
  pub fn uses_flakes(&self) -> bool {
    self.installable.uses_flakes(CommandContext::Darwin)
  }
}

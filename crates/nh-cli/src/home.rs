use nh_installable::InstallableArgs;
use nh_remote::RemoteHost;
use pound::Parse;

use crate::common::CommonRebuildArgs;

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

#[derive(Debug, Parse)]
pub struct HomeRebuildArgs {
  #[pound(flatten)]
  pub common: CommonRebuildArgs,

  #[pound(flatten)]
  pub update_args: crate::common::UpdateArgs,

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
  #[deprecated(
    since = "4.5.0",
    note = "activation output is always shown, and this flag will be removed \
            in 5.0"
  )]
  #[pound(long, env = "NH_SHOW_ACTIVATION_LOGS")]
  pub show_activation_logs: bool,

  /// Build the configuration on a different host over SSH
  #[pound(long, parse = str::parse::<RemoteHost>)]
  pub build_host: Option<RemoteHost>,
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

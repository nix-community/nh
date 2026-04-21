use std::path::PathBuf;

use clap::{Args, Subcommand};

// Needed a struct to have multiple sub-subcommands
#[derive(Debug, Clone, Args)]
pub struct CleanProxy {
  #[clap(subcommand)]
  pub command: CleanMode,
}

#[derive(Debug, Clone, Subcommand)]
/// Enhanced nix cleanup
pub enum CleanMode {
  /// Clean all profiles
  All(CleanArgs),
  /// Clean the current user's profiles
  User(CleanArgs),
  /// Clean a specific profile
  Profile(CleanProfileArgs),
}

#[derive(Args, Clone, Debug)]
pub struct CleanArgs {
  #[arg(long, short, default_value = "1")]
  /// At least keep this number of generations
  pub keep: u32,

  #[arg(long, short = 'K', default_value = "0h")]
  /// At least keep gcroots and generations in this time range since now.
  ///
  /// See the documentation of humantime for possible formats: <https://docs.rs/humantime/latest/humantime/fn.parse_duration.html>
  pub keep_since: humantime::Duration,

  /// Only print actions, without performing them
  #[arg(long, short = 'n')]
  pub dry: bool,

  /// Ask for confirmation
  #[arg(long, short)]
  pub ask: bool,

  /// Don't run nix store --gc
  #[arg(long = "no-gc", alias = "nogc")]
  pub no_gc: bool,

  /// Don't clean gcroots
  #[arg(long = "no-gcroots", alias = "nogcroots")]
  pub no_gcroots: bool,

  /// Run nix-store --optimise after gc
  #[arg(long)]
  pub optimise: bool,

  /// Pass --max to nix store gc
  #[arg(long)]
  pub max: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct CleanProfileArgs {
  #[command(flatten)]
  pub common: CleanArgs,

  /// Which profile to clean
  pub profile: PathBuf,
}

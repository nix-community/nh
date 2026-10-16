use std::path::PathBuf;

use pound::Parse;

// Needed a struct to have multiple sub-subcommands
#[derive(Debug, Clone, Parse)]
pub struct CleanProxy {
  #[pound(subcommand)]
  pub command: CleanMode,
}

#[derive(Debug, Clone, Parse)]
/// Enhanced nix cleanup
pub enum CleanMode {
  /// Clean all profiles
  All {
    #[pound(flatten)]
    args: CleanArgs,
  },
  /// Clean the current user's profiles
  User {
    #[pound(flatten)]
    args: CleanArgs,
  },
  /// Clean a specific profile
  Profile {
    #[pound(flatten)]
    args: CleanProfileArgs,
  },
}

#[derive(Clone, Debug, Parse)]
pub struct CleanArgs {
  /// At least keep this number of generations
  #[pound(long, short, default = "1")]
  pub keep: u32,

  /// At least keep gcroots and generations in this time range since now.
  ///
  /// See the documentation of humantime for possible formats: <https://docs.rs/humantime/latest/humantime/fn.parse_duration.html>
  #[pound(long, short = 'K', default = "0h", parse = str::parse::<humantime::Duration>)]
  pub keep_since:     humantime::Duration,
  /// Allow removing the selected generation when it exceeds retention limits.
  /// This can leave the profile symlink dangling.
  #[pound(long)]
  pub delete_current: bool,

  /// Only print actions, without performing them
  #[pound(long, short = 'n')]
  pub dry: bool,

  /// Ask for confirmation
  #[pound(long, short, env = "NH_ASK")]
  pub ask: bool,

  /// Don't run nix store --gc
  #[pound(long = "no-gc", alias = "nogc")]
  pub no_gc: bool,

  /// Don't clean gcroots
  #[pound(long = "no-gcroots", alias = "nogcroots")]
  pub no_gcroots: bool,

  /// Don't clean direnv gcroots
  #[pound(long = "no-direnv", alias = "nodirenv")]
  pub no_direnv: bool,

  /// Run nix-store --optimise after gc
  #[pound(long)]
  pub optimise: bool,

  /// Pass --max to nix store gc
  #[pound(long)]
  pub max: Option<String>,

  /// Keep at least one gcroot per direnv project
  #[pound(long)]
  pub keep_one: bool,

  /// Cross filesystem boundaries when scanning gcroots
  #[pound(long, short = 'x')]
  pub cross_filesystems: bool,
}

#[derive(Debug, Clone, Parse)]
pub struct CleanProfileArgs {
  #[pound(flatten)]
  pub common: CleanArgs,

  /// Which profile to clean
  pub profile: PathBuf,
}

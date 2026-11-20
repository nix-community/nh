use std::path::PathBuf;

use nh_core::args::{self, DiffType};
use pound::{Choice, FromArg, Parse, ValueEnum, ValueError};

use crate::installable::InstallableArgs;

#[derive(Debug, Parse)]
pub struct CommonRebuildArgs {
  /// Only print actions, without performing them
  #[pound(long, short = 'n')]
  pub dry: bool,

  /// Ask for confirmation
  #[pound(long, short, env = "NH_ASK")]
  pub ask: bool,

  #[pound(flatten)]
  pub installable: InstallableArgs,

  /// Don't use rom for the build process
  #[pound(long)]
  pub no_rom: bool,

  /// Deprecated alias for `--no-rom`
  #[pound(long, hidden)]
  pub no_nom: bool,

  /// Path to save the result link, defaults to using a temporary directory
  #[pound(long, short)]
  pub out_link: Option<PathBuf>,

  /// Whether to display a package diff
  #[pound(long, short, default = "auto")]
  pub diff: Diff,

  #[pound(flatten)]
  pub passthrough: NixBuildPassthroughArgs,
}

/// When to display a package diff, and what to show in it.
#[derive(Debug, Clone, Copy)]
pub struct Diff(pub args::Diff);

#[derive(ValueEnum)]
enum DiffWord {
  /// Display package diff, and only warn if it fails
  Auto,
  /// Display package diff, and abort if it fails
  Always,
  /// Never display package diff
  Never,
  /// Also show packages whose version did not change, e.g. always,full
  Full,
}

impl FromArg for Diff {
  const POSSIBLE: Option<&'static [Choice]> = DiffWord::POSSIBLE;

  fn from_arg(raw: &str) -> Result<Self, ValueError> {
    let mut mode = None;
    let mut full = false;

    for word in raw.split(',') {
      let parsed = match DiffWord::from_arg(word)? {
        DiffWord::Auto => DiffType::Auto,
        DiffWord::Always => DiffType::Always,
        DiffWord::Never => DiffType::Never,
        DiffWord::Full => {
          full = true;
          continue;
        },
      };

      if mode.replace(parsed).is_some() {
        return Err(ValueError::new(
          raw,
          "expected at most one of auto, always or never",
        ));
      }
    }

    let mode = mode.unwrap_or(DiffType::Auto);
    if full && mode == DiffType::Never {
      return Err(ValueError::new(raw, "full has no effect with never"));
    }

    Ok(Self(args::Diff { mode, full }))
  }
}

#[derive(Debug, Default, Parse)]
pub struct NixBuildPassthroughArgs {
  /// Number of concurrent jobs Nix should run
  #[pound(long, short = 'j')]
  pub max_jobs: Option<usize>,

  /// Number of cores Nix should utilize
  #[pound(long)]
  pub cores: Option<usize>,

  /// Logging format used by Nix
  #[pound(long)]
  pub log_format: Option<String>,

  /// Continue building despite encountering errors
  #[pound(long, short = 'k')]
  pub keep_going: bool,

  /// Keep build outputs from failed builds
  #[pound(long, short = 'K')]
  pub keep_failed: bool,

  /// Attempt to build locally if substituters fail
  #[pound(long)]
  pub fallback: bool,

  /// Repair corrupted store paths
  #[pound(long)]
  pub repair: bool,

  /// Explicitly define remote builders
  #[pound(long)]
  pub builders: Option<String>,

  /// Paths to include
  #[pound(long, short = 'I')]
  pub include: Vec<String>,

  /// Print build logs directly to stdout
  #[pound(long, short = 'L')]
  pub print_build_logs: bool,

  /// Display tracebacks on errors
  #[pound(long, short = 't')]
  pub show_trace: bool,

  /// Accept configuration from flakes
  #[pound(long)]
  pub accept_flake_config: bool,

  /// Refresh flakes to the latest revision
  #[pound(long)]
  pub refresh: bool,

  /// Allow impure builds
  #[pound(long)]
  pub impure: bool,

  /// Build without internet access
  #[pound(long)]
  pub offline: bool,

  /// Prohibit network usage
  #[pound(long)]
  pub no_net: bool,

  /// Recreate the flake.lock file entirely
  #[pound(long)]
  pub recreate_lock_file: bool,

  /// Do not update the flake.lock file
  #[pound(long)]
  pub no_update_lock_file: bool,

  /// Do not write a lock file
  #[pound(long)]
  pub no_write_lock_file: bool,

  /// Do not use registries
  #[pound(long = "no-use-registries")]
  pub no_use_registries: bool,

  /// Do not use registries (deprecated, use --no-use-registries)
  #[pound(long, alias = "no-registries")]
  pub no_registries: bool,

  /// Commit the lock file after updates
  #[pound(long)]
  pub commit_lock_file: bool,

  /// Suppress build output
  #[pound(long, short = 'Q')]
  pub no_build_output: bool,

  /// Use substitutes when copying
  #[pound(long)]
  pub use_substitutes: bool,

  /// Output results in JSON format
  #[pound(long)]
  pub json: bool,

  /// Set a Nix configuration option (may be given multiple times)
  #[pound(long, num_args = 2, value_name = "NAME VALUE")]
  pub option: Vec<String>,

  /// Override a specific flake input (may be given multiple times)
  #[pound(long, num_args = 2, value_name = "INPUT FLAKE_URL")]
  pub override_input: Vec<String>,
}

#[derive(Debug, Parse)]
pub struct UpdateArgs {
  #[pound(short = 'u', long = "update", conflicts_with = update_input)]
  /// Update all flake inputs
  pub update_all: bool,

  #[pound(short = 'U', long = "update-input", conflicts_with = update_all)]
  /// Update the specified flake input(s)
  pub update_input: Option<Vec<String>>,
}

use std::path::PathBuf;

use clap::{Args, ValueEnum};
use nh_installable::Installable;

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
  pub passthrough: nh_passthrough::NixBuildPassthroughArgs,
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

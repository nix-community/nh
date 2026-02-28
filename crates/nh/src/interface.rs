use anstyle::Style;
use clap::{Args, Parser, Subcommand, ValueEnum, builder::Styles};
use clap_verbosity_flag::InfoLevel;
use nh_command::ElevationStrategy;
use nh_core::checks::{FeatureRequirements, NoFeatures};
use nh_nixos;

use crate::Result;

const fn make_style() -> Styles {
  Styles::plain().header(Style::new().bold()).literal(
    Style::new()
      .bold()
      .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Yellow))),
  )
}

#[derive(Parser, Debug)]
#[command(
    version,
    about,
    long_about = None,
    styles=make_style(),
    propagate_version = false,
    help_template = "
{name} {version}
{about-with-newline}
{usage-heading} {usage}

{all-args}{after-help}
"
)]
/// Yet another nix helper
pub struct Main {
  #[command(flatten)]
  /// Increase logging verbosity, can be passed multiple times for
  /// more detailed logs.
  pub verbosity: clap_verbosity_flag::Verbosity<InfoLevel>,

  #[arg(
    short,
    long,
    global = true,
    env = "NH_ELEVATION_STRATEGY",
    value_hint = clap::ValueHint::CommandName,
    alias = "elevation-program"
  )]
  /// Choose the privilege elevation strategy.
  ///
  /// Can be a path to an elevation program (e.g., /usr/bin/sudo),
  /// or one of: 'none' (no elevation),
  /// 'passwordless' (use elevation without password prompt for remote hosts
  /// with NOPASSWD configured), or 'auto' (automatically detect available
  /// elevation programs in order: doas, sudo, run0, pkexec)
  pub elevation_strategy: Option<nh_command::ElevationStrategyArg>,

  #[command(subcommand)]
  pub command: NHCommand,
}

#[derive(Subcommand, Debug)]
#[command(disable_help_subcommand = true)]
pub enum NHCommand {
  Os(nh_nixos::args::OsArgs),
  Home(nh_home::args::HomeArgs),
  Darwin(nh_darwin::args::DarwinArgs),
  Search(nh_search::args::SearchArgs),
  Clean(nh_clean::args::CleanProxy),
}

impl NHCommand {
  #[must_use]
  pub fn get_feature_requirements(&self) -> Box<dyn FeatureRequirements> {
    match self {
      Self::Os(args) => args.get_feature_requirements(),
      Self::Home(args) => args.get_feature_requirements(),
      Self::Darwin(args) => args.get_feature_requirements(),
      Self::Search(..) | Self::Clean(..) => Box::new(NoFeatures),
    }
  }

  pub fn run(self, elevation: ElevationStrategy) -> Result<()> {
    // Check features specific to this command
    let requirements = self.get_feature_requirements();
    requirements.check_features()?;

    match self {
      Self::Os(args) => args.run(elevation),
      Self::Search(args) => args.run(),
      Self::Clean(proxy) => proxy.command.run(elevation),
      Self::Home(args) => args.run(),
      Self::Darwin(args) => args.run(elevation),
    }
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
pub struct UpdateArgs {
  #[arg(short = 'u', long = "update", conflicts_with = "update_input")]
  /// Update all flake inputs
  pub update_all: bool,

  #[arg(short = 'U', long = "update-input", conflicts_with = "update_all")]
  /// Update the specified flake input(s)
  pub update_input: Option<Vec<String>>,
}

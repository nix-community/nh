use anstyle::Style;
use clap::{Parser, Subcommand, builder::Styles};
use clap_verbosity_flag::InfoLevel;
use nh_core::{
  checks::{FeatureRequirements, NoFeatures},
  command::ElevationStrategy,
};
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
  pub elevation_strategy: Option<nh_core::command::ElevationStrategyArg>,

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

  /// Run the selected subcommand.
  ///
  /// # Errors
  ///
  /// Returns an error if required Nix features are unavailable or if the
  /// selected subcommand fails.
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

#[cfg(test)]
mod tests {
  use std::{env, ffi::OsString};

  use clap::{Parser, error::ErrorKind};
  use nh_clean::args::CleanMode;
  use serial_test::serial;

  use super::{Main, NHCommand};

  struct EnvGuard(Option<OsString>);

  impl EnvGuard {
    fn new() -> Self {
      Self(env::var_os("NH_ASK"))
    }
  }

  impl Drop for EnvGuard {
    fn drop(&mut self) {
      unsafe {
        match &self.0 {
          Some(value) => env::set_var("NH_ASK", value),
          None => env::remove_var("NH_ASK"),
        }
      }
    }
  }

  #[test]
  #[serial]
  fn nh_ask_parses_boolish_environment_values() -> clap::error::Result<()> {
    let _guard = EnvGuard::new();

    for (value, expected) in
      [("1", true), ("true", true), ("0", false), ("false", false)]
    {
      unsafe {
        env::set_var("NH_ASK", value);
      }
      let parsed = Main::try_parse_from(["nh", "clean", "all"])?;
      let ask = match parsed.command {
        NHCommand::Clean(proxy) => {
          match proxy.command {
            CleanMode::All(args) => Some(args.ask),
            _ => None,
          }
        },
        _ => None,
      };
      assert_eq!(ask, Some(expected));
    }

    unsafe {
      env::set_var("NH_ASK", "invalid");
    }
    assert!(matches!(
      Main::try_parse_from(["nh", "clean", "all"]),
      Err(error) if error.kind() == ErrorKind::ValueValidation
    ));

    Ok(())
  }
}

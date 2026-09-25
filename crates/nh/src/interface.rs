use anstyle::Style;
use clap::{Parser, Subcommand, builder::Styles};
use clap_verbosity_flag::{InfoLevel, Verbosity, VerbosityFilter};
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

fn verbosity_arg(verbosity: Verbosity<InfoLevel>) -> Option<&'static str> {
  match verbosity.filter() {
    VerbosityFilter::Off => Some("-qqq"),
    VerbosityFilter::Error => Some("-qq"),
    VerbosityFilter::Warn => Some("-q"),
    VerbosityFilter::Info => None,
    VerbosityFilter::Debug => Some("-v"),
    VerbosityFilter::Trace => Some("-vv"),
  }
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

  #[command(name = "__privileged-activate", hide = true)]
  PrivilegedActivate(nh_nixos::args::PrivilegedActivationArgs),

  #[command(name = "__privileged-rollback", hide = true)]
  PrivilegedRollback(nh_nixos::args::PrivilegedRollbackArgs),

  #[command(name = "__privileged-darwin", hide = true)]
  PrivilegedDarwin(nh_darwin::args::PrivilegedDarwinArgs),
}

impl NHCommand {
  #[must_use]
  pub fn get_feature_requirements(&self) -> Box<dyn FeatureRequirements> {
    match self {
      Self::Os(args) => args.get_feature_requirements(),
      Self::Home(args) => args.get_feature_requirements(),
      Self::Darwin(args) => args.get_feature_requirements(),
      Self::Search(..)
      | Self::Clean(..)
      | Self::PrivilegedActivate(..)
      | Self::PrivilegedRollback(..)
      | Self::PrivilegedDarwin(..) => Box::new(NoFeatures),
    }
  }

  /// Run the selected subcommand.
  ///
  /// # Errors
  ///
  /// Returns an error if required Nix features are unavailable or if the
  /// selected subcommand fails.
  pub fn run(
    self,
    elevation: ElevationStrategy,
    verbosity: Verbosity<InfoLevel>,
  ) -> Result<()> {
    // Check features specific to this command
    let requirements = self.get_feature_requirements();
    requirements.check_features()?;

    match self {
      Self::Os(args) => args.run(elevation, verbosity_arg(verbosity)),
      Self::Search(args) => args.run(),
      Self::Clean(proxy) => proxy.command.run(elevation),
      Self::Home(args) => args.run(),
      Self::Darwin(args) => args.run(elevation, verbosity_arg(verbosity)),
      Self::PrivilegedActivate(args) => args.run(),
      Self::PrivilegedRollback(args) => args.run(),
      Self::PrivilegedDarwin(args) => args.run(),
    }
  }
}

#[cfg(test)]
#[expect(clippy::panic, reason = "Fine in tests")]
mod tests {
  use std::{env, ffi::OsString, path::PathBuf};

  use clap::{Parser, error::ErrorKind};
  use clap_verbosity_flag::{InfoLevel, Verbosity};
  use nh_clean::args::CleanMode;
  use nh_darwin::args::PrivilegedDarwinArgs;
  use nh_nixos::args::{
    PrivilegedActivationAction,
    PrivilegedActivationArgs,
    PrivilegedRollbackArgs,
  };
  use serial_test::serial;

  use super::{Main, NHCommand, verbosity_arg};

  struct EnvGuard {
    name:  &'static str,
    value: Option<OsString>,
  }

  impl EnvGuard {
    fn new(name: &'static str) -> Self {
      Self {
        name,
        value: env::var_os(name),
      }
    }
  }
  impl Drop for EnvGuard {
    fn drop(&mut self) {
      unsafe {
        match &self.value {
          Some(value) => env::set_var(self.name, value),
          None => env::remove_var(self.name),
        }
      }
    }
  }

  #[test]
  #[serial]
  fn nh_ask_parses_boolish_environment_values() -> clap::error::Result<()> {
    let _guard = EnvGuard::new("NH_ASK");

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

  #[test]
  fn verbosity_arg_uses_effective_level() {
    let cases = [
      (Verbosity::<InfoLevel>::new(0, 3), Some("-qqq")),
      (Verbosity::<InfoLevel>::new(0, 2), Some("-qq")),
      (Verbosity::<InfoLevel>::new(0, 1), Some("-q")),
      (Verbosity::<InfoLevel>::default(), None),
      (Verbosity::<InfoLevel>::new(1, 0), Some("-v")),
      (Verbosity::<InfoLevel>::new(2, 0), Some("-vv")),
    ];

    for (verbosity, expected) in cases {
      assert_eq!(verbosity_arg(verbosity), expected);
    }
  }

  #[test]
  fn privileged_args_round_trip() -> clap::error::Result<()> {
    let activation = PrivilegedActivationArgs {
      action:                         PrivilegedActivationAction::Switch,
      switch_to_configuration:        PathBuf::from(
        "/nix/store/config/bin/switch-to-configuration",
      ),
      system:                         PathBuf::from("/nix/store/system"),
      continue_on_activation_failure: true,
      install_bootloader:             true,
      show_activation_logs:           true,
      nix_args:                       vec![
        "--option".into(),
        "substituters".into(),
        "https://cache.example.org".into(),
      ],
    };
    let mut argv = vec![OsString::from("nh")];
    argv.extend(activation.command_args(Some("-v")));
    let parsed = Main::try_parse_from(argv)?;
    let NHCommand::PrivilegedActivate(reparsed) = parsed.command else {
      panic!("expected privileged activation arguments");
    };
    assert!(matches!(
      reparsed.action,
      PrivilegedActivationAction::Switch
    ));
    assert_eq!(
      reparsed.switch_to_configuration,
      activation.switch_to_configuration
    );
    assert_eq!(reparsed.system, activation.system);
    assert!(reparsed.continue_on_activation_failure);
    assert!(reparsed.install_bootloader);
    assert!(reparsed.show_activation_logs);
    assert_eq!(reparsed.nix_args, activation.nix_args);

    let rollback = PrivilegedRollbackArgs {
      target_profile:          PathBuf::from(
        "/nix/var/nix/profiles/system-42-link",
      ),
      previous_profile:        Some(PathBuf::from(
        "/nix/var/nix/profiles/system-41-link",
      )),
      switch_to_configuration: PathBuf::from(
        "/nix/store/config/bin/switch-to-configuration",
      ),
    };
    let mut argv = vec![OsString::from("nh")];
    argv.extend(rollback.command_args(None));
    let parsed = Main::try_parse_from(argv)?;
    let NHCommand::PrivilegedRollback(reparsed) = parsed.command else {
      panic!("expected privileged rollback arguments");
    };
    assert_eq!(reparsed.target_profile, rollback.target_profile);
    assert_eq!(reparsed.previous_profile, rollback.previous_profile);
    assert_eq!(
      reparsed.switch_to_configuration,
      rollback.switch_to_configuration
    );

    let darwin = PrivilegedDarwinArgs {
      system:               PathBuf::from("/nix/store/system"),
      activate:             true,
      show_activation_logs: true,
      nix_args:             vec![
        "--option".into(),
        "builders".into(),
        "".into(),
      ],
    };
    let mut argv = vec![OsString::from("nh")];
    argv.extend(darwin.command_args(Some("-q")));
    let parsed = Main::try_parse_from(argv)?;
    let NHCommand::PrivilegedDarwin(reparsed) = parsed.command else {
      panic!("expected privileged Darwin arguments");
    };
    assert_eq!(reparsed.system, darwin.system);
    assert!(reparsed.activate);
    assert!(reparsed.show_activation_logs);
    assert_eq!(reparsed.nix_args, darwin.nix_args);

    Ok(())
  }
}

pub mod clean;
pub mod common;
pub mod darwin;
pub mod home;
pub mod installable;
pub mod os;
pub mod search;

use pound::Parse;

#[derive(Parse, Debug, Clone, Copy, Default)]
pub struct Verbosity {
  /// Increase logging verbosity
  #[pound(short = 'v', long, count, global)]
  pub verbose: u32,

  /// Decrease logging verbosity
  #[pound(short = 'q', long, count, global, conflicts_with = verbose)]
  pub quiet: u32,
}

/// Yet another nix helper
#[derive(Parse, Debug)]
#[pound(name = "nh")]
pub struct Main {
  /// Increase logging verbosity, can be passed multiple times for
  /// more detailed logs.
  #[pound(flatten)]
  pub verbosity: Verbosity,

  /// Choose the privilege elevation strategy.
  ///
  /// Can be a path to an elevation program (e.g., /usr/bin/sudo),
  /// or one of: 'none' (no elevation),
  /// 'passwordless' (use elevation without password prompt for remote hosts
  /// with NOPASSWD configured), 'empty-password' (use remote sudo with an
  /// empty password and a forwarded SSH agent, for PAM modules such as
  /// `pam_rssh`), or 'auto' (automatically detect available elevation programs
  /// in order: doas, sudo, run0, pkexec)
  #[pound(
    short,
    long,
    global,
    env = "NH_ELEVATION_STRATEGY",
    alias = "elevation-program",
    parse = str::parse::<nh_core::command::ElevationStrategyArg>
  )]
  pub elevation_strategy: Option<nh_core::command::ElevationStrategyArg>,

  #[pound(subcommand)]
  pub command: NHCommand,
}

#[derive(Parse, Debug)]
pub enum NHCommand {
  /// NixOS functionality
  ///
  /// Implements functionality mostly around but not exclusive to nixos-rebuild
  Os {
    #[pound(flatten)]
    args: os::OsArgs,
  },
  /// Home-manager functionality
  Home {
    #[pound(flatten)]
    args: home::HomeArgs,
  },
  /// Nix-darwin functionality
  ///
  /// Implements functionality mostly around but not exclusive to darwin-rebuild
  Darwin {
    #[pound(flatten)]
    args: darwin::DarwinArgs,
  },
  /// Searches packages or NixOS/home-manager options via search.nixos.org,
  /// or a local SPAM database
  Search {
    #[pound(flatten)]
    args: search::SearchArgs,
  },
  /// Enhanced nix cleanup
  Clean {
    #[pound(flatten)]
    args: clean::CleanProxy,
  },
}

#[cfg(test)]
mod tests {
  use std::{env, ffi::OsString};

  use pound::{ErrorKind, Parse};
  use serial_test::serial;

  use super::{Main, NHCommand, clean::CleanMode, home, os};

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
  fn nh_ask_parses_boolish_environment_values() -> Result<(), pound::Error> {
    let _guard = EnvGuard::new();
    let clean_all_ask = |parsed: Main| {
      match parsed.command {
        NHCommand::Clean { args: proxy } => {
          match proxy.command {
            CleanMode::All { args } => Some(args.ask),
            CleanMode::User { .. } | CleanMode::Profile { .. } => None,
          }
        },
        NHCommand::Os { .. }
        | NHCommand::Home { .. }
        | NHCommand::Darwin { .. }
        | NHCommand::Search { .. } => None,
      }
    };

    for (value, expected) in [
      ("1", true),
      ("yes", true),
      ("true", true),
      ("0", false),
      ("no", false),
      ("false", false),
    ] {
      unsafe {
        env::set_var("NH_ASK", value);
      }
      let parsed = Main::try_parse_from(["clean", "all"])?;
      assert_eq!(clean_all_ask(parsed), Some(expected));
    }

    unsafe {
      env::set_var("NH_ASK", "invalid");
    }
    assert!(matches!(
      Main::try_parse_from(["clean", "all"]),
      Err(error) if matches!(error.kind, ErrorKind::Value { .. })
    ));
    let parsed = Main::try_parse_from(["clean", "all", "--ask"])?;
    assert_eq!(clean_all_ask(parsed), Some(true));

    Ok(())
  }

  #[test]
  #[serial]
  fn passthrough_requires_separator() {
    assert!(matches!(
      Main::try_parse_from(["os", "build", ".", "unexpected"]),
      Err(error) if matches!(error.kind, ErrorKind::UnexpectedPositional(_))
    ));
  }

  #[test]
  #[serial]
  fn passthrough_without_installable_keeps_every_word()
  -> Result<(), pound::Error> {
    let parsed = Main::try_parse_from([
      "home", "build", "--", "--json", "--option", "sandbox", "false",
    ])?;
    assert!(matches!(
      parsed.command,
      NHCommand::Home {
        args: home::HomeArgs {
          subcommand: home::HomeSubcommand::Build { args },
        },
      } if args.extra_args == ["--json", "--option", "sandbox", "false"]
    ));
    Ok(())
  }

  #[test]
  #[serial]
  fn passthrough_preserves_installable_and_option_boundaries()
  -> Result<(), pound::Error> {
    let parsed = Main::try_parse_from([
      "os", "build", ".", "--option", "sandbox", "false", "--", "--cores", "8",
    ])?;
    assert!(matches!(
      parsed.command,
      NHCommand::Os {
        args: os::OsArgs {
          subcommand: os::OsSubcommand::Build { args },
        },
      } if args.common.passthrough.option == ["sandbox", "false"]
        && args.extra_args == ["--cores", "8"]
    ));
    Ok(())
  }

  #[test]
  #[serial]
  fn verbosity_is_global_and_mutually_exclusive() -> Result<(), pound::Error> {
    let parsed = Main::try_parse_from(["clean", "all", "-vv"])?;
    assert_eq!(parsed.verbosity.verbose, 2);
    assert_eq!(parsed.verbosity.quiet, 0);
    assert!(
      matches!(Main::try_parse_from(["-v", "clean", "all", "-q"]), Err(error) if matches!(error.kind, ErrorKind::Conflict { .. }))
    );
    Ok(())
  }
}

use std::{env, fs, path::PathBuf};

use clap::{Arg, ArgAction, Args, FromArgMatches, error::ErrorKind};
use tracing::debug;
use yansi::{Color, Paint};

// Reference: https://nix.dev/manual/nix/2.18/command-ref/new-cli/nix

#[derive(Debug, Clone)]
pub enum Installable {
  Flake {
    reference: String,
    attribute: String,
  },
  File {
    path:      PathBuf,
    attribute: String,
  },
  Store {
    path: PathBuf,
  },
  Expression {
    expression: String,
    attribute:  String,
  },
}

impl FromArgMatches for Installable {
  fn from_arg_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
    let mut matches = matches.clone();
    Self::from_arg_matches_mut(&mut matches)
  }

  fn from_arg_matches_mut(
    matches: &mut clap::ArgMatches,
  ) -> Result<Self, clap::Error> {
    let installable = matches.get_one::<String>("installable");
    let file = matches.get_one::<String>("file");
    let expr = matches.get_one::<String>("expr");

    if let Some(i) = installable {
      let canonincal = fs::canonicalize(i);

      if let Ok(p) = canonincal {
        if p.starts_with("/nix/store") {
          return Ok(Self::Store { path: p });
        }
      }
    }

    if let Some(f) = file {
      return Ok(Self::File {
        path:      PathBuf::from(f),
        attribute: installable.cloned().unwrap_or_default(),
      });
    }

    if let Some(e) = expr {
      return Ok(Self::Expression {
        expression: e.to_string(),
        attribute:  installable.cloned().unwrap_or_default(),
      });
    }

    if let Some(i) = installable {
      let mut elems = i.splitn(2, '#');
      let reference = elems.next().unwrap().to_owned();
      return Ok(Self::Flake {
        reference,
        attribute: elems
          .next()
          .map(std::string::ToString::to_string)
          .unwrap_or_default(),
      });
    }

    // Env var parsing & fallbacks
    fn parse_flake_env(var: &str) -> Option<Installable> {
      env::var(var).ok().map(|f| {
        let mut elems = f.splitn(2, '#');
        Installable::Flake {
          reference: elems.next().unwrap().to_owned(),
          attribute: elems
            .next()
            .map(std::string::ToString::to_string)
            .unwrap_or_default(),
        }
      })
    }

    // Command-specific flake env vars
    if let Ok(subcommand) = env::var("NH_CURRENT_COMMAND") {
      debug!("Current subcommand: {subcommand:?}");
      let env_var = match subcommand.as_str() {
        "os" => "NH_OS_FLAKE",
        "home" => "NH_HOME_FLAKE",
        "darwin" => "NH_DARWIN_FLAKE",
        _ => "",
      };

      if !env_var.is_empty() {
        if let Some(installable) = parse_flake_env(env_var) {
          return Ok(installable);
        }
      }
    }

    // General flake env fallbacks
    for var in &[
      "NH_FLAKE",
      "NH_OS_FLAKE",
      "NH_HOME_FLAKE",
      "NH_DARWIN_FLAKE",
    ] {
      if let Some(installable) = parse_flake_env(var) {
        return Ok(installable);
      }
    }

    if let Ok(f) = env::var("NH_FILE") {
      return Ok(Self::File {
        path:      PathBuf::from(f),
        attribute: env::var("NH_ATTRP").unwrap_or_default(),
      });
    }

    Err(clap::Error::new(ErrorKind::TooFewValues))
  }

  fn update_from_arg_matches(
    &mut self,
    _matches: &clap::ArgMatches,
  ) -> Result<(), clap::Error> {
    todo!()
  }
}

impl Args for Installable {
  fn augment_args(cmd: clap::Command) -> clap::Command {
    let nh_flake = env::var("NH_FLAKE").unwrap_or_default();
    let nh_os_flake = env::var("NH_OS_FLAKE").unwrap_or_default();
    let nh_home_flake = env::var("NH_HOME_FLAKE").unwrap_or_default();
    let nh_darwin_flake = env::var("NH_DARWIN_FLAKE").unwrap_or_default();
    let nh_file = env::var("NH_FILE").unwrap_or_default();
    let nh_attr = env::var("NH_ATTR").unwrap_or_default();

    let long_help = format!(
      r"Which installable to use.
Nix accepts various kinds of installables:

[FLAKEREF[#ATTRPATH]]
    Flake reference with an optional attribute path.
    [env: NH_FLAKE={nh_flake}]
    [env: NH_OS_FLAKE={nh_os_flake}]
    [env: NH_HOME_FLAKE={nh_home_flake}]
    [env: NH_DARWIN_FLAKE={nh_darwin_flake}]

{f_short}, {f_long} <FILE> [ATTRPATH]
    Path to file with an optional attribute path.
    [env: NH_FILE={nh_file}]
    [env: NH_ATTRP={nh_attr}]

{e_short}, {e_long} <EXPR> [ATTRPATH]
    Nix expression with an optional attribute path.

[PATH]
    Path or symlink to a /nix/store path
",
      f_short = "-f".yellow(),
      f_long = "--file".yellow(),
      e_short = "-e".yellow(),
      e_long = "--expr".yellow(),
    );

    cmd
      .arg(
        Arg::new("file")
          .short('f')
          .long("file")
          .action(ArgAction::Set)
          .hide(true),
      )
      .arg(
        Arg::new("expr")
          .short('E')
          .long("expr")
          .action(ArgAction::Set)
          .hide(true)
          .conflicts_with("file"),
      )
      .arg(
        Arg::new("installable")
          .action(ArgAction::Set)
          .value_name("INSTALLABLE")
          .help("Which installable to use")
          .long_help(long_help),
      )
  }

  fn augment_args_for_update(cmd: clap::Command) -> clap::Command {
    Self::augment_args(cmd)
  }
}

impl Installable {
  #[must_use]
  pub fn to_args(&self) -> Vec<String> {
    match self {
      Self::Flake {
        reference,
        attribute,
      } => {
        vec![format!("{reference}#{attribute}")]
      },
      Self::File { path, attribute } => {
        vec![
          String::from("--file"),
          path.to_str().unwrap().to_string(),
          attribute.to_string(),
        ]
      },
      Self::Expression {
        expression,
        attribute,
      } => {
        vec![
          String::from("--expr"),
          expression.to_string(),
          attribute.to_string(),
        ]
      },
      Self::Store { path } => vec![path.to_str().unwrap().to_string()],
    }
  }
}

#[test]
fn test_installable_to_args() {
  assert_eq!(
    (Installable::Flake {
      reference: String::from("w"),
      attribute: ["x", "y.z"].into_iter().map(str::to_string).collect(),
    })
    .to_args(),
    vec![r#"w#x."y.z""#]
  );

  assert_eq!(
    (Installable::File {
      path:      PathBuf::from("w"),
      attribute: ["x", "y.z"].into_iter().map(str::to_string).collect(),
    })
    .to_args(),
    vec!["--file", "w", r#"x."y.z""#]
  );
}

impl Installable {
  #[must_use]
  pub const fn str_kind(&self) -> &str {
    match self {
      Self::Flake { .. } => "flake",
      Self::File { .. } => "file",
      Self::Store { .. } => "store path",
      Self::Expression { .. } => "expression",
    }
  }
}

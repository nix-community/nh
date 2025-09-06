use std::{env, fs, path::PathBuf};

use clap::{Arg, ArgAction, Args, FromArgMatches, error::ErrorKind};
use color_eyre::owo_colors::OwoColorize;
use tracing::debug;

// Reference: https://nix.dev/manual/nix/2.18/command-ref/new-cli/nix

#[derive(Debug, Clone)]
pub enum Installable {
  Flake {
    reference: String,
    attribute: Vec<String>,
  },
  File {
    path:      PathBuf,
    attribute: Vec<String>,
  },
  Store {
    path: PathBuf,
  },
  Expression {
    expression: String,
    attribute:  Vec<String>,
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
        attribute: parse_attribute(installable.cloned().unwrap_or_default()),
      });
    }

    if let Some(e) = expr {
      return Ok(Self::Expression {
        expression: e.to_string(),
        attribute:  parse_attribute(installable.cloned().unwrap_or_default()),
      });
    }

    if let Some(i) = installable {
      let mut elems = i.splitn(2, '#');
      let reference = elems.next().unwrap().to_owned();
      return Ok(Self::Flake {
        reference,
        attribute: parse_attribute(
          elems
            .next()
            .map(std::string::ToString::to_string)
            .unwrap_or_default(),
        ),
      });
    }

    // Env var parsing & fallbacks
    fn parse_flake_env(var: &str) -> Option<Installable> {
      env::var(var).ok().map(|f| {
        let mut elems = f.splitn(2, '#');
        Installable::Flake {
          reference: elems.next().unwrap().to_owned(),
          attribute: parse_attribute(
            elems
              .next()
              .map(std::string::ToString::to_string)
              .unwrap_or_default(),
          ),
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
        attribute: parse_attribute(env::var("NH_ATTRP").unwrap_or_default()),
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
          .conflicts_with("file")
          .hide(true)
          .action(ArgAction::Set),
      )
      .arg(
        Arg::new("installable")
          .action(ArgAction::Set)
          .value_name("INSTALLABLE")
          .help("Which installable to use")
          .long_help(format!(
            r"Which installable to use.
Nix accepts various kinds of installables:

[FLAKEREF[#ATTRPATH]]
    Flake reference with an optional attribute path.
    [env: NH_FLAKE={}]
    [env: NH_OS_FLAKE={}]
    [env: NH_HOME_FLAKE={}]
    [env: NH_DARWIN_FLAKE={}]

{}, {} <FILE> [ATTRPATH]
    Path to file with an optional attribute path.
    [env: NH_FILE={}]
    [env: NH_ATTRP={}]

{}, {} <EXPR> [ATTRPATH]
    Nix expression with an optional attribute path.

[PATH]
    Path or symlink to a /nix/store path
",
            env::var("NH_FLAKE").unwrap_or_default(),
            env::var("NH_OS_FLAKE").unwrap_or_default(),
            env::var("NH_HOME_FLAKE").unwrap_or_default(),
            env::var("NH_DARWIN_FLAKE").unwrap_or_default(),
            "-f".yellow(),
            "--file".yellow(),
            env::var("NH_FILE").unwrap_or_default(),
            env::var("NH_ATTR").unwrap_or_default(),
            "-e".yellow(),
            "--expr".yellow(),
          )),
      )
  }

  fn augment_args_for_update(cmd: clap::Command) -> clap::Command {
    Self::augment_args(cmd)
  }
}

// TODO: should handle quoted attributes, like foo."bar.baz" -> ["foo",
// "bar.baz"] maybe use chumsky?
pub fn parse_attribute<S>(s: S) -> Vec<String>
where
  S: AsRef<str>,
{
  let s = s.as_ref();
  let mut res = Vec::new();

  if s.is_empty() {
    return res;
  }

  let mut in_quote = false;

  let mut elem = String::new();
  for char in s.chars() {
    match char {
      '.' => {
        if in_quote {
          elem.push(char);
        } else {
          res.push(elem.clone());
          elem = String::new();
        }
      },
      '"' => {
        in_quote = !in_quote;
      },
      _ => elem.push(char),
    }
  }

  res.push(elem);

  assert!(!in_quote, "Failed to parse attribute: {s}");

  res
}

#[test]
fn test_parse_attribute() {
  assert_eq!(parse_attribute(r"foo.bar"), vec!["foo", "bar"]);
  assert_eq!(parse_attribute(r#"foo."bar.baz""#), vec!["foo", "bar.baz"]);
  let v: Vec<String> = vec![];
  assert_eq!(parse_attribute(""), v);
}

impl Installable {
  #[must_use]
  pub fn to_args(&self) -> Vec<String> {
    let mut res = Vec::new();
    match self {
      Self::Flake {
        reference,
        attribute,
      } => {
        res.push(format!("{reference}#{}", join_attribute(attribute)));
      },
      Self::File { path, attribute } => {
        res.push(String::from("--file"));
        res.push(path.to_str().unwrap().to_string());
        res.push(join_attribute(attribute));
      },
      Self::Expression {
        expression,
        attribute,
      } => {
        res.push(String::from("--expr"));
        res.push(expression.to_string());
        res.push(join_attribute(attribute));
      },
      Self::Store { path } => res.push(path.to_str().unwrap().to_string()),
    }

    res
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

fn join_attribute<I>(attribute: I) -> String
where
  I: IntoIterator,
  I::Item: AsRef<str>,
{
  let mut res = String::new();
  let mut first = true;
  for elem in attribute {
    if first {
      first = false;
    } else {
      res.push('.');
    }

    let s = elem.as_ref();

    if s.contains('.') {
      res.push_str(&format!(r#""{s}""#));
    } else {
      res.push_str(s);
    }
  }

  res
}

#[test]
fn test_join_attribute() {
  assert_eq!(join_attribute(vec!["foo", "bar"]), "foo.bar");
  assert_eq!(join_attribute(vec!["foo", "bar.baz"]), r#"foo."bar.baz""#);
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

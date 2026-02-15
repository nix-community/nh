use std::{env, fs, path::PathBuf};

use clap::{Arg, ArgAction, Args, FromArgMatches, error::ErrorKind};
use tracing::debug;
use yansi::{Color, Paint};

// Reference: https://nix.dev/manual/nix/2.18/command-ref/new-cli/nix

/// Command context for resolving installable env var priority
#[derive(Debug, Clone, Copy)]
pub enum CommandContext {
  Os,
  Home,
  Darwin,
}

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

  /// Represents a deferred resolution of a missing installable.
  /// This variant should be resolved to a concrete installable before use.
  Unspecified,
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
        expression: e.clone(),
        attribute:  parse_attribute(installable.cloned().unwrap_or_default()),
      });
    }

    if let Some(i) = installable {
      let mut elems = i.splitn(2, '#');
      let reference = elems
        .next()
        .ok_or_else(|| {
          clap::Error::raw(
            ErrorKind::ValueValidation,
            "Invalid installable format: missing reference",
          )
        })?
        .to_owned();
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

    Ok(Self::Unspecified)
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
            Paint::new("-f").fg(Color::Yellow),
            Paint::new("--file").fg(Color::Yellow),
            env::var("NH_FILE").unwrap_or_default(),
            env::var("NH_ATTR").unwrap_or_default(),
            Paint::new("-e").fg(Color::Yellow),
            Paint::new("--expr").fg(Color::Yellow),
          )),
      )
  }

  fn augment_args_for_update(cmd: clap::Command) -> clap::Command {
    Self::augment_args(cmd)
  }
}

// TODO: `parse_attribute` should handle quoted attributes, such as:
// foo."bar.baz" -> ["foo", "bar.baz"]
// Maybe we want to use chumsky for this?
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
        if let Some(path_str) = path.to_str() {
          res.push(String::from("--file"));
          res.push(path_str.to_string());
          res.push(join_attribute(attribute));
        } else {
          // Return empty args if path contains invalid UTF-8
          return Vec::new();
        }
      },
      Self::Expression {
        expression,
        attribute,
      } => {
        res.push(String::from("--expr"));
        res.push(expression.clone());
        res.push(join_attribute(attribute));
      },
      Self::Store { path } => {
        if let Some(path_str) = path.to_str() {
          res.push(path_str.to_string());
        } else {
          // Return empty args if path contains invalid UTF-8
          return Vec::new();
        }
      },

      Self::Unspecified => {
        unreachable!(
          "Unspecified installable should have been resolved before calling \
           to_args"
        )
      },
    }

    res
  }

  /// Resolves an installable, checking environment variables in
  /// command-specific priority order.
  ///
  /// If the installable is not Unspecified, returns it as-is.
  /// Otherwise, checks env vars in priority order based on the command context:
  /// - For NixOS: NH_OS_FLAKE, then NH_FLAKE
  /// - For Home: NH_HOME_FLAKE, then NH_FLAKE
  /// - For Darwin: NH_DARWIN_FLAKE, then NH_FLAKE
  ///
  /// Returns an error if no installable could be resolved and no default is
  /// available.
  pub fn resolve(self, context: CommandContext) -> color_eyre::Result<Self> {
    match self {
      Self::Unspecified => {
        // Check command-specific env var first
        let specific_var = match context {
          CommandContext::Os => "NH_OS_FLAKE",
          CommandContext::Home => "NH_HOME_FLAKE",
          CommandContext::Darwin => "NH_DARWIN_FLAKE",
        };
        if let Ok(flake) = env::var(specific_var) {
          debug!("Using {specific_var}: {flake}");
          let mut elems = flake.splitn(2, '#');
          let reference = elems.next().ok_or_else(|| {
            color_eyre::eyre::eyre!("{specific_var} missing reference part")
          })?;
          return Ok(Self::Flake {
            reference: reference.to_owned(),
            attribute: parse_attribute(
              elems
                .next()
                .map(std::string::ToString::to_string)
                .unwrap_or_default(),
            ),
          });
        }

        // Fall back to NH_FILE
        if let Ok(file) = env::var("NH_FILE") {
          debug!("Using NH_FILE: {file}");
          return Ok(Self::File {
            path:      PathBuf::from(file),
            attribute: parse_attribute(
              env::var("NH_ATTRP").unwrap_or_default(),
            ),
          });
        }

        // Fall back to NH_FLAKE
        if let Ok(flake) = env::var("NH_FLAKE") {
          debug!("Using NH_FLAKE: {flake}");
          let mut elems = flake.splitn(2, '#');
          let reference = elems.next().ok_or_else(|| {
            color_eyre::eyre::eyre!("NH_FLAKE missing reference part")
          })?;
          return Ok(Self::Flake {
            reference: reference.to_owned(),
            attribute: parse_attribute(
              elems
                .next()
                .map(std::string::ToString::to_string)
                .unwrap_or_default(),
            ),
          });
        }

        // Return Unspecified - caller should try default resolution
        Ok(Self::Unspecified)
      },
      other => Ok(other),
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

enum FallbackError {
  NotFound,
  PermissionDenied(PathBuf),
  Io(std::io::Error),
}

/// Resolves a fallback flake directory.
///
/// # Returns
///
/// The resolved path to use as a flake reference. This handles three cases:
///
/// 1. Directory is a symlink -> returns the resolved directory path
/// 2. Directory is real but flake.nix is a symlink → returns the parent
///    directory of the resolved flake.nix
/// 3. Both are real -> returns the original directory
///
/// # Errors
///
/// Returns an error if:
///
/// - The directory does not exist
/// - The directory exists but does not contain a flake.nix file
/// - Permission is denied accessing the directory or flake.nix
/// - Any other I/O error occurs
fn resolve_fallback_flake_dir(
  dir: &std::path::Path,
) -> Result<PathBuf, FallbackError> {
  use std::io::ErrorKind;

  // Check if the directory itself is a symlink
  let dir_is_symlink = dir.is_symlink();

  // Resolve the directory path
  let resolved_dir = match fs::canonicalize(dir) {
    Ok(p) => p,
    Err(e) => {
      return match e.kind() {
        ErrorKind::NotFound => Err(FallbackError::NotFound),
        ErrorKind::PermissionDenied => {
          Err(FallbackError::PermissionDenied(dir.to_path_buf()))
        },
        _ => Err(FallbackError::Io(e)),
      };
    },
  };

  // If the directory itself was a symlink, use the resolved directory
  if dir_is_symlink {
    let flake_path = resolved_dir.join("flake.nix");
    return match fs::metadata(&flake_path) {
      Ok(m) if m.is_file() => Ok(resolved_dir),
      Ok(_) => Err(FallbackError::NotFound),
      Err(e) => {
        match e.kind() {
          ErrorKind::NotFound => Err(FallbackError::NotFound),
          ErrorKind::PermissionDenied => {
            Err(FallbackError::PermissionDenied(flake_path))
          },
          _ => Err(FallbackError::Io(e)),
        }
      },
    };
  }

  // Directory is real, check flake.nix
  let flake_path = resolved_dir.join("flake.nix");

  // Check if flake.nix is a symlink
  if flake_path.is_symlink() {
    // Resolve the symlink to get the actual flake.nix location
    match fs::canonicalize(&flake_path) {
      Ok(resolved_flake) => {
        // Use the parent directory of the resolved flake.nix
        resolved_flake
          .parent()
          .map_or(Err(FallbackError::NotFound), |parent| {
            Ok(parent.to_path_buf())
          })
      },
      Err(e) => {
        match e.kind() {
          ErrorKind::NotFound => Err(FallbackError::NotFound),
          ErrorKind::PermissionDenied => {
            Err(FallbackError::PermissionDenied(flake_path))
          },
          _ => Err(FallbackError::Io(e)),
        }
      },
    }
  } else {
    // flake.nix is a real file, check it exists
    match fs::metadata(&flake_path) {
      Ok(m) if m.is_file() => Ok(resolved_dir),
      Ok(_) => Err(FallbackError::NotFound),
      Err(e) => {
        match e.kind() {
          ErrorKind::NotFound => Err(FallbackError::NotFound),
          ErrorKind::PermissionDenied => {
            Err(FallbackError::PermissionDenied(flake_path))
          },
          _ => Err(FallbackError::Io(e)),
        }
      },
    }
  }
}

const FALLBACK_HELP_HINT: &str =
  "See 'man nh' or https://github.com/nix-community/nh for more details.";

impl Installable {
  #[must_use]
  pub const fn str_kind(&self) -> &str {
    match self {
      Self::Flake { .. } => "flake",
      Self::File { .. } => "file",
      Self::Store { .. } => "store path",
      Self::Expression { .. } => "expression",
      Self::Unspecified => "unspecified",
    }
  }

  /// Attempts to find a default installable for `NixOS` builds.
  ///
  /// Checks if `/etc/nixos/flake.nix` exists and returns a flake installable
  /// pointing to it if found. If the directory is a symlink, it is resolved
  /// to its canonical path. Otherwise, returns an error with instructions on
  /// how to specify an installable.
  ///
  /// # Errors
  ///
  /// Returns an error if:
  ///
  /// - No flake is found at `/etc/nixos/flake.nix`
  /// - Permission is denied accessing the path
  /// - The resolved path contains invalid UTF-8
  pub fn try_find_default_for_os() -> color_eyre::Result<Self> {
    use tracing::warn;

    let default_dir = std::path::Path::new("/etc/nixos");

    match resolve_fallback_flake_dir(default_dir) {
      Ok(resolved) => {
        warn!(
          "No installable was specified, falling back to {}",
          resolved.display()
        );
        Ok(Self::Flake {
          reference: resolved
            .to_str()
            .ok_or_else(|| {
              color_eyre::eyre::eyre!(
                "Resolved path {} contains invalid UTF-8",
                resolved.display()
              )
            })?
            .to_string(),
          attribute: vec![],
        })
      },
      Err(FallbackError::PermissionDenied(path)) => {
        Err(color_eyre::eyre::eyre!(
          "Permission denied accessing {}.\nPlease either:\n- Pass a flake \
           path as an argument (e.g., 'nh os switch .')\n- Set the NH_FLAKE \
           environment variable\n- Set the NH_OS_FLAKE environment \
           variable\n\n{}",
          path.display(),
          FALLBACK_HELP_HINT
        ))
      },
      Err(FallbackError::Io(e)) => {
        Err(color_eyre::eyre::eyre!(
          "I/O error accessing {}: {}\n\n{}",
          default_dir.display(),
          e,
          FALLBACK_HELP_HINT
        ))
      },
      Err(FallbackError::NotFound) => {
        Err(color_eyre::eyre::eyre!(
          "No installable specified and no flake found at \
           {}/flake.nix.\nPlease either:\n- Pass a flake path as an argument \
           (e.g., 'nh os switch .')\n- Set the NH_FLAKE environment \
           variable\n- Set the NH_OS_FLAKE environment variable\n\n{}",
          default_dir.display(),
          FALLBACK_HELP_HINT
        ))
      },
    }
  }

  /// Attempts to find a default installable for Home Manager builds.
  ///
  /// Checks if `$HOME/.config/home-manager/flake.nix` exists and returns a
  /// flake installable pointing to it if found. If the directory is a
  /// symlink, it is resolved to its canonical path. Otherwise, returns an
  /// error with instructions on how to specify an installable.
  ///
  /// # Errors
  ///
  /// Returns an error if:
  ///
  /// - The `HOME` environment variable is not set
  /// - No flake is found at `$HOME/.config/home-manager/flake.nix`
  /// - Permission is denied accessing the path
  /// - The resolved path contains invalid UTF-8
  pub fn try_find_default_for_home() -> color_eyre::Result<Self> {
    use tracing::warn;

    let home = env::var("HOME").map_err(|_| {
      color_eyre::eyre::eyre!("HOME environment variable not set")
    })?;
    let default_dir = PathBuf::from(&home).join(".config/home-manager");

    match resolve_fallback_flake_dir(&default_dir) {
      Ok(resolved) => {
        warn!(
          "No installable was specified, falling back to {}",
          resolved.display()
        );
        Ok(Self::Flake {
          reference: resolved
            .to_str()
            .ok_or_else(|| {
              color_eyre::eyre::eyre!(
                "Resolved path {} contains invalid UTF-8",
                resolved.display()
              )
            })?
            .to_string(),
          attribute: vec![],
        })
      },
      Err(FallbackError::PermissionDenied(path)) => {
        Err(color_eyre::eyre::eyre!(
          "Permission denied accessing {}.\nPlease either:\n- Pass a flake \
           path as an argument (e.g., 'nh home switch .')\n- Set the NH_FLAKE \
           environment variable\n- Set the NH_HOME_FLAKE environment \
           variable\n\n{}",
          path.display(),
          FALLBACK_HELP_HINT
        ))
      },
      Err(FallbackError::Io(e)) => {
        Err(color_eyre::eyre::eyre!(
          "I/O error accessing {}: {}\n\n{}",
          default_dir.display(),
          e,
          FALLBACK_HELP_HINT
        ))
      },
      Err(FallbackError::NotFound) => {
        Err(color_eyre::eyre::eyre!(
          "No installable specified and no flake found at \
           {}/flake.nix.\nPlease either:\n- Pass a flake path as an argument \
           (e.g., 'nh home switch .')\n- Set the NH_FLAKE environment \
           variable\n- Set the NH_HOME_FLAKE environment variable\n\n{}",
          default_dir.display(),
          FALLBACK_HELP_HINT
        ))
      },
    }
  }

  /// Attempts to find a default installable for Darwin builds.
  ///
  /// Checks if `/etc/nix-darwin/flake.nix` exists and returns a flake
  /// installable pointing to it if found. If the directory is a symlink,
  /// it is resolved to its canonical path. Otherwise, returns an error with
  /// instructions on how to specify an installable.
  ///
  /// # Errors
  ///
  /// Returns an error if:
  ///
  /// - No flake is found at `/etc/nix-darwin/flake.nix`
  /// - Permission is denied accessing the path
  /// - The resolved path contains invalid UTF-8
  pub fn try_find_default_for_darwin() -> color_eyre::Result<Self> {
    use tracing::warn;

    let default_dir = std::path::Path::new("/etc/nix-darwin");

    match resolve_fallback_flake_dir(default_dir) {
      Ok(resolved) => {
        warn!(
          "No installable was specified, falling back to {}",
          resolved.display()
        );
        Ok(Self::Flake {
          reference: resolved
            .to_str()
            .ok_or_else(|| {
              color_eyre::eyre::eyre!(
                "Resolved path {} contains invalid UTF-8",
                resolved.display()
              )
            })?
            .to_string(),
          attribute: vec![],
        })
      },
      Err(FallbackError::PermissionDenied(path)) => {
        Err(color_eyre::eyre::eyre!(
          "Permission denied accessing {}.\nPlease either:\n- Pass a flake \
           path as an argument (e.g., 'nh darwin switch .')\n- Set the \
           NH_FLAKE environment variable\n- Set the NH_DARWIN_FLAKE \
           environment variable\n\n{}",
          path.display(),
          FALLBACK_HELP_HINT
        ))
      },
      Err(FallbackError::Io(e)) => {
        Err(color_eyre::eyre::eyre!(
          "I/O error accessing {}: {}\n\n{}",
          default_dir.display(),
          e,
          FALLBACK_HELP_HINT
        ))
      },
      Err(FallbackError::NotFound) => {
        Err(color_eyre::eyre::eyre!(
          "No installable specified and no flake found at \
           {}/flake.nix.\nPlease either:\n- Pass a flake path as an argument \
           (e.g., 'nh darwin switch .')\n- Set the NH_FLAKE environment \
           variable\n- Set the NH_DARWIN_FLAKE environment variable\n\n{}",
          default_dir.display(),
          FALLBACK_HELP_HINT
        ))
      },
    }
  }
}

#[cfg(test)]
mod tests {
  use std::env;

  use serial_test::serial;

  use super::*;

  #[test]
  fn test_resolve_non_unspecified_returns_unchanged() {
    // Test that non-Unspecified installables are returned as-is
    let flake = Installable::Flake {
      reference: String::from("/path/to/flake"),
      attribute: vec![String::from("host")],
    };
    let resolved = flake.clone().resolve(CommandContext::Os).unwrap();
    assert_eq!(flake.to_args(), resolved.to_args());

    let file = Installable::File {
      path:      PathBuf::from("/path/to/file.nix"),
      attribute: vec![String::from("config")],
    };
    let resolved = file.clone().resolve(CommandContext::Home).unwrap();
    assert_eq!(file.to_args(), resolved.to_args());

    let store = Installable::Store {
      path: PathBuf::from("/nix/store/abc"),
    };
    let resolved = store.clone().resolve(CommandContext::Darwin).unwrap();
    assert_eq!(store.to_args(), resolved.to_args());

    let expr = Installable::Expression {
      expression: String::from("{ pkgs }: pkgs.hello"),
      attribute:  vec![],
    };
    let resolved = expr.clone().resolve(CommandContext::Os).unwrap();
    assert_eq!(expr.to_args(), resolved.to_args());
  }

  #[test]
  #[serial]
  fn test_resolve_os_context_uses_nh_os_flake() {
    // Set only NH_OS_FLAKE
    unsafe {
      env::set_var("NH_OS_FLAKE", "/etc/nixos#myhost");
      env::remove_var("NH_FLAKE");
      env::remove_var("NH_HOME_FLAKE");
      env::remove_var("NH_DARWIN_FLAKE");
    }

    let resolved = Installable::Unspecified
      .resolve(CommandContext::Os)
      .unwrap();
    match resolved {
      Installable::Flake {
        reference,
        attribute,
      } => {
        assert_eq!(reference, "/etc/nixos");
        assert_eq!(attribute, vec!["myhost"]);
      },
      _ => panic!("Expected Flake, got {:?}", resolved),
    }

    unsafe {
      env::remove_var("NH_OS_FLAKE");
    }
  }

  #[test]
  #[serial]
  fn test_resolve_os_context_prefers_os_flake_over_generic() {
    // Set both NH_OS_FLAKE and NH_FLAKE
    unsafe {
      env::set_var("NH_OS_FLAKE", "/etc/nixos#myhost");
      env::set_var("NH_FLAKE", "/home/user/flake#other");
    }

    let resolved = Installable::Unspecified
      .resolve(CommandContext::Os)
      .unwrap();
    match resolved {
      Installable::Flake {
        reference,
        attribute,
      } => {
        assert_eq!(reference, "/etc/nixos");
        assert_eq!(attribute, vec!["myhost"]);
      },
      _ => panic!("Expected Flake, got {:?}", resolved),
    }

    unsafe {
      env::remove_var("NH_OS_FLAKE");
      env::remove_var("NH_FLAKE");
    }
  }

  #[test]
  #[serial]
  fn test_resolve_os_context_falls_back_to_nh_flake() {
    // Set only NH_FLAKE
    unsafe {
      env::remove_var("NH_OS_FLAKE");
      env::set_var("NH_FLAKE", "/home/user/flake#fallback");
    }

    let resolved = Installable::Unspecified
      .resolve(CommandContext::Os)
      .unwrap();
    match resolved {
      Installable::Flake {
        reference,
        attribute,
      } => {
        assert_eq!(reference, "/home/user/flake");
        assert_eq!(attribute, vec!["fallback"]);
      },
      _ => panic!("Expected Flake, got {:?}", resolved),
    }

    unsafe {
      env::remove_var("NH_FLAKE");
    }
  }

  #[test]
  #[serial]
  fn test_resolve_home_context_uses_nh_home_flake() {
    // Set only NH_HOME_FLAKE
    unsafe {
      env::set_var("NH_HOME_FLAKE", "~/.config/home-manager#myuser");
      env::remove_var("NH_FLAKE");
    }

    let resolved = Installable::Unspecified
      .resolve(CommandContext::Home)
      .unwrap();
    match resolved {
      Installable::Flake {
        reference,
        attribute,
      } => {
        assert_eq!(reference, "~/.config/home-manager");
        assert_eq!(attribute, vec!["myuser"]);
      },
      _ => panic!("Expected Flake, got {:?}", resolved),
    }

    unsafe {
      env::remove_var("NH_HOME_FLAKE");
    }
  }

  #[test]
  #[serial]
  fn test_resolve_home_context_prefers_home_flake_over_generic() {
    // Set both NH_HOME_FLAKE and NH_FLAKE
    unsafe {
      env::set_var("NH_HOME_FLAKE", "~/.config/home-manager#myuser");
      env::set_var("NH_FLAKE", "/other/flake#other");
    }

    let resolved = Installable::Unspecified
      .resolve(CommandContext::Home)
      .unwrap();
    match resolved {
      Installable::Flake {
        reference,
        attribute,
      } => {
        assert_eq!(reference, "~/.config/home-manager");
        assert_eq!(attribute, vec!["myuser"]);
      },
      _ => panic!("Expected Flake, got {:?}", resolved),
    }

    unsafe {
      env::remove_var("NH_HOME_FLAKE");
      env::remove_var("NH_FLAKE");
    }
  }

  #[test]
  #[serial]
  fn test_resolve_darwin_context_uses_nh_darwin_flake() {
    // Set only NH_DARWIN_FLAKE
    unsafe {
      env::set_var("NH_DARWIN_FLAKE", "/etc/nix-darwin#macbook");
      env::remove_var("NH_FLAKE");
    }

    let resolved = Installable::Unspecified
      .resolve(CommandContext::Darwin)
      .unwrap();
    match resolved {
      Installable::Flake {
        reference,
        attribute,
      } => {
        assert_eq!(reference, "/etc/nix-darwin");
        assert_eq!(attribute, vec!["macbook"]);
      },
      _ => panic!("Expected Flake, got {:?}", resolved),
    }

    unsafe {
      env::remove_var("NH_DARWIN_FLAKE");
    }
  }

  #[test]
  #[serial]
  fn test_resolve_darwin_context_prefers_darwin_flake_over_generic() {
    // Set both NH_DARWIN_FLAKE and NH_FLAKE
    unsafe {
      env::set_var("NH_DARWIN_FLAKE", "/etc/nix-darwin#macbook");
      env::set_var("NH_FLAKE", "/other/flake#other");
    }

    let resolved = Installable::Unspecified
      .resolve(CommandContext::Darwin)
      .unwrap();
    match resolved {
      Installable::Flake {
        reference,
        attribute,
      } => {
        assert_eq!(reference, "/etc/nix-darwin");
        assert_eq!(attribute, vec!["macbook"]);
      },
      _ => panic!("Expected Flake, got {:?}", resolved),
    }

    unsafe {
      env::remove_var("NH_DARWIN_FLAKE");
      env::remove_var("NH_FLAKE");
    }
  }

  #[test]
  #[serial]
  fn test_resolve_no_env_vars_returns_unspecified() {
    // Clear all env vars
    unsafe {
      env::remove_var("NH_FLAKE");
      env::remove_var("NH_OS_FLAKE");
      env::remove_var("NH_HOME_FLAKE");
      env::remove_var("NH_DARWIN_FLAKE");
    }

    let resolved = Installable::Unspecified
      .resolve(CommandContext::Os)
      .unwrap();
    assert!(matches!(resolved, Installable::Unspecified));

    let resolved = Installable::Unspecified
      .resolve(CommandContext::Home)
      .unwrap();
    assert!(matches!(resolved, Installable::Unspecified));

    let resolved = Installable::Unspecified
      .resolve(CommandContext::Darwin)
      .unwrap();
    assert!(matches!(resolved, Installable::Unspecified));
  }

  #[test]
  #[serial]
  fn test_resolve_with_empty_attribute() {
    // Test flake without attribute
    unsafe {
      env::set_var("NH_OS_FLAKE", "/etc/nixos");
    }

    let resolved = Installable::Unspecified
      .resolve(CommandContext::Os)
      .unwrap();
    match resolved {
      Installable::Flake {
        reference,
        attribute,
      } => {
        assert_eq!(reference, "/etc/nixos");
        assert!(attribute.is_empty());
      },
      _ => panic!("Expected Flake, got {:?}", resolved),
    }

    unsafe {
      env::remove_var("NH_OS_FLAKE");
    }
  }

  #[test]
  #[serial]
  fn test_resolve_with_nested_attribute() {
    // Test flake with chained attribute
    unsafe {
      env::set_var(
        "NH_HOME_FLAKE",
        "~/.config/home-manager#homeConfigurations.user",
      );
    }

    let resolved = Installable::Unspecified
      .resolve(CommandContext::Home)
      .unwrap();
    match resolved {
      Installable::Flake {
        reference,
        attribute,
      } => {
        assert_eq!(reference, "~/.config/home-manager");
        assert_eq!(attribute, vec!["homeConfigurations", "user"]);
      },
      _ => panic!("Expected Flake, got {:?}", resolved),
    }

    unsafe {
      env::remove_var("NH_HOME_FLAKE");
    }
  }

  #[test]
  #[serial]
  fn test_resolve_command_specific_isolation() {
    // Ensure command contexts are isolated - setting NH_HOME_FLAKE should not
    // affect OS context
    unsafe {
      env::set_var("NH_HOME_FLAKE", "~/.config/home-manager#user");
      env::remove_var("NH_OS_FLAKE");
      env::remove_var("NH_FLAKE");
    }

    // OS context should not pick up NH_HOME_FLAKE
    let resolved = Installable::Unspecified
      .resolve(CommandContext::Os)
      .unwrap();
    assert!(matches!(resolved, Installable::Unspecified));

    // But Home context should
    let resolved = Installable::Unspecified
      .resolve(CommandContext::Home)
      .unwrap();
    match resolved {
      Installable::Flake {
        reference,
        attribute,
      } => {
        assert_eq!(reference, "~/.config/home-manager");
        assert_eq!(attribute, vec!["user"]);
      },
      _ => panic!("Expected Flake, got {:?}", resolved),
    }

    unsafe {
      env::remove_var("NH_HOME_FLAKE");
    }
  }
}

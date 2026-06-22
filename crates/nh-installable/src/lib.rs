use std::{
  env,
  fs,
  path::{Path, PathBuf},
};

use clap::{Arg, ArgAction, Args, FromArgMatches, ValueHint, error::ErrorKind};
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

impl CommandContext {
  const fn specific_flake_env_var(self) -> &'static str {
    match self {
      Self::Os => "NH_OS_FLAKE",
      Self::Home => "NH_HOME_FLAKE",
      Self::Darwin => "NH_DARWIN_FLAKE",
    }
  }
}

#[derive(Debug, Clone)]
pub enum InstallableArgs {
  Specified(Installable),
  Unspecified,
}

enum EnvInstallableSource {
  SpecificFlake {
    env_var: &'static str,
    value:   String,
  },
  File {
    path:      String,
    attribute: String,
  },
  GenericFlake(String),
}

impl EnvInstallableSource {
  const fn uses_flakes(&self) -> bool {
    match self {
      Self::SpecificFlake { value, .. } | Self::GenericFlake(value) => {
        !value.is_empty()
      },
      Self::File { .. } => false,
    }
  }

  fn into_installable(self) -> color_eyre::Result<Installable> {
    match self {
      Self::SpecificFlake { env_var, value } => {
        debug!("Using {env_var}: {value}");
        flake_from_env_var(env_var, &value)
      },
      Self::File { path, attribute } => {
        debug!("Using NH_FILE: {path}");
        Ok(Installable::File {
          path:      PathBuf::from(path),
          attribute: parse_attribute(&attribute)
            .map_err(|err| color_eyre::eyre::eyre!("NH_ATTRP {err}"))?,
        })
      },
      Self::GenericFlake(value) => {
        debug!("Using NH_FLAKE: {value}");
        flake_from_env_var("NH_FLAKE", &value)
      },
    }
  }
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
}

impl FromArgMatches for InstallableArgs {
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
      let canonical = fs::canonicalize(i);

      if let Ok(p) = canonical
        && p.starts_with("/nix/store")
      {
        return Ok(Self::Specified(Installable::Store { path: p }));
      }
    }

    if let Some(f) = file {
      let attribute = parse_attribute(installable.map_or("", String::as_str))
        .map_err(|err| {
        clap::Error::raw(
          ErrorKind::ValueValidation,
          format!("attribute path {err}"),
        )
      })?;
      return Ok(Self::Specified(Installable::File {
        path: PathBuf::from(f),
        attribute,
      }));
    }

    if let Some(e) = expr {
      let attribute = parse_attribute(installable.map_or("", String::as_str))
        .map_err(|err| {
        clap::Error::raw(
          ErrorKind::ValueValidation,
          format!("attribute path {err}"),
        )
      })?;
      return Ok(Self::Specified(Installable::Expression {
        expression: e.clone(),
        attribute,
      }));
    }

    if let Some(i) = installable {
      let (reference, attribute) = parse_flake_reference(i).map_err(|err| {
        clap::Error::raw(
          ErrorKind::ValueValidation,
          format!("installable argument {err}"),
        )
      })?;
      return Ok(Self::Specified(Installable::Flake {
        reference,
        attribute,
      }));
    }

    Ok(Self::Unspecified)
  }

  fn update_from_arg_matches(
    &mut self,
    matches: &clap::ArgMatches,
  ) -> Result<(), clap::Error> {
    *self = Self::from_arg_matches(matches)?;
    Ok(())
  }
}

impl Args for InstallableArgs {
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
          .value_hint(ValueHint::AnyPath)
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
            env::var("NH_ATTRP").unwrap_or_default(),
            Paint::new("-e").fg(Color::Yellow),
            Paint::new("--expr").fg(Color::Yellow),
          )),
      )
  }

  fn augment_args_for_update(cmd: clap::Command) -> clap::Command {
    Self::augment_args(cmd)
  }
}

fn parse_attribute(s: &str) -> Result<Vec<String>, &'static str> {
  let mut res = Vec::new();

  if s.is_empty() {
    return Ok(res);
  }

  let mut in_quote = false;
  let mut elem = String::new();

  let mut chars = s.chars();
  while let Some(char) = chars.next() {
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
      '\\' if in_quote => {
        let escaped = chars
          .next()
          .ok_or("contains an incomplete quoted attribute escape")?;
        elem.push(escaped);
      },
      _ => elem.push(char),
    }
  }

  res.push(elem);

  if in_quote {
    return Err("contains an unclosed quoted attribute segment");
  }

  Ok(res)
}

fn parse_flake_reference(
  value: &str,
) -> Result<(String, Vec<String>), &'static str> {
  // CLI installables and NH_*_FLAKE values share the same flakeref grammar.
  // Reject empty references here so Nix never turns `""` or `#attr` into an
  // implicit search from the current directory.
  if value.is_empty() {
    return Err("is empty. Set it to a flake reference or remove it.");
  }

  let (reference, attribute) = value
    .split_once('#')
    .map_or((value, ""), |(reference, attribute)| (reference, attribute));

  if reference.is_empty() {
    return Err("missing reference part before `#`");
  }

  let attribute = parse_attribute(attribute)?;
  Ok((reference.to_owned(), attribute))
}

#[test]
fn test_parse_attribute() {
  assert_eq!(
    parse_attribute(r"foo.bar"),
    Ok(vec!["foo".to_string(), "bar".to_string()])
  );
  assert_eq!(
    parse_attribute(r#"foo."bar.baz""#),
    Ok(vec!["foo".to_string(), "bar.baz".to_string()])
  );
  assert_eq!(
    parse_attribute(r#"foo."bar\"baz"."bar\\baz""#),
    Ok(vec![
      "foo".to_string(),
      "bar\"baz".to_string(),
      "bar\\baz".to_string()
    ])
  );
  let v: Vec<String> = vec![];
  assert_eq!(parse_attribute(""), Ok(v));
  assert!(parse_attribute(r#"foo."bar"#).is_err());
  assert!(parse_attribute(r#"foo."bar\"#).is_err());
}

impl InstallableArgs {
  /// Returns whether the parsed CLI input or non-empty flake environment
  /// variables select flake mode for the command context.
  #[must_use]
  pub fn uses_flakes(&self, context: CommandContext) -> bool {
    // Empty flake env vars are invalid inputs. Do not count them as feature
    // requirements here; resolution reports the targeted validation error.
    match self {
      Self::Specified(Installable::Flake { .. }) => true,
      Self::Specified(_) => false,
      Self::Unspecified => {
        env_installable_source(context)
          .is_some_and(|source| source.uses_flakes())
      },
    }
  }

  /// Resolves an installable from the CLI value or environment.
  ///
  /// If an installable was supplied on the CLI, returns it as-is. Otherwise,
  /// checks env vars in priority order based on the command context:
  /// - The command-specific flake env var: `NH_OS_FLAKE`, `NH_HOME_FLAKE`, or
  ///   `NH_DARWIN_FLAKE`
  /// - `NH_FILE`, with `NH_ATTRP` as the optional attribute path
  /// - `NH_FLAKE`
  ///
  /// Returns `None` when no installable environment variable is set.
  ///
  /// # Errors
  ///
  /// Returns an error when a configured flake environment variable is
  /// malformed.
  fn resolve(
    self,
    context: CommandContext,
  ) -> color_eyre::Result<Option<Installable>> {
    match self {
      Self::Unspecified => {
        env_installable_source(context)
          .map(EnvInstallableSource::into_installable)
          .transpose()
      },
      Self::Specified(installable) => Ok(Some(installable)),
    }
  }

  /// Resolve an installable and fall back to the command-specific default when
  /// the installable is unspecified.
  ///
  /// Explicit local flake references are validated before command execution. A
  /// supplied local path must point at the directory containing `flake.nix`;
  /// `nh` does not let Nix search parent directories for it.
  ///
  /// # Errors
  ///
  /// Returns an error when environment resolution fails, when a local flake
  /// reference does not point at a flake directory, or when no default
  /// installable can be found for the command context.
  pub fn resolve_or_default(
    self,
    context: CommandContext,
  ) -> color_eyre::Result<Installable> {
    let Some(installable) = self.resolve(context)? else {
      return default_installable_for(context);
    };

    installable.validate_local_flake_ref(context)?;
    Ok(installable)
  }
}

fn env_installable_source(
  context: CommandContext,
) -> Option<EnvInstallableSource> {
  let specific_var = context.specific_flake_env_var();
  if let Ok(value) = env::var(specific_var) {
    return Some(EnvInstallableSource::SpecificFlake {
      env_var: specific_var,
      value,
    });
  }

  if let Ok(path) = env::var("NH_FILE") {
    return Some(EnvInstallableSource::File {
      path,
      attribute: env::var("NH_ATTRP").unwrap_or_default(),
    });
  }

  if let Ok(value) = env::var("NH_FLAKE") {
    return Some(EnvInstallableSource::GenericFlake(value));
  }

  None
}

fn default_installable_for(
  context: CommandContext,
) -> color_eyre::Result<Installable> {
  match context {
    CommandContext::Os => try_find_default_for_os(),
    CommandContext::Home => try_find_default_for_home(),
    CommandContext::Darwin => try_find_default_for_darwin(),
  }
}

fn flake_from_env_var(
  name: &str,
  value: &str,
) -> color_eyre::Result<Installable> {
  let (reference, attribute) = parse_flake_reference(value)
    .map_err(|err| color_eyre::eyre::eyre!("{name} {err}"))?;
  Ok(Installable::Flake {
    reference,
    attribute,
  })
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
    }

    res
  }

  fn validate_local_flake_ref(
    &self,
    context: CommandContext,
  ) -> color_eyre::Result<()> {
    let Self::Flake { reference, .. } = self else {
      return Ok(());
    };

    let Some(path) = local_flake_reference_path(reference) else {
      return Ok(());
    };

    // For explicit local refs, fail before invoking Nix so the error points at
    // the bad configuration instead of Nix's parent-directory search.
    match resolve_fallback_flake_dir(&path) {
      Ok(_) => Ok(()),
      Err(FallbackError::NotFound) => {
        Err(color_eyre::eyre::eyre!(
          "Flake reference `{}` points to local path `{}`, but that path does \
           not exist or does not contain a flake.nix file.\nPass an existing \
           flake path or update NH_FLAKE/{} if this value came from the \
           environment.",
          reference,
          path.display(),
          context.specific_flake_env_var()
        ))
      },
      Err(FallbackError::PermissionDenied(path)) => {
        Err(color_eyre::eyre::eyre!(
          "Permission denied accessing {} while checking flake reference `{}`.",
          path.display(),
          reference
        ))
      },
      Err(FallbackError::Io(source)) => {
        Err(color_eyre::eyre::eyre!(
          "I/O error checking flake reference `{}` at {}: {}",
          reference,
          path.display(),
          source
        ))
      },
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

    if s.is_empty() || s.contains(['.', '"', '\\']) {
      res.push('"');
      for char in s.chars() {
        match char {
          '"' | '\\' => {
            res.push('\\');
            res.push(char);
          },
          _ => res.push(char),
        }
      }
      res.push('"');
    } else {
      res.push_str(s);
    }
  }

  res
}

fn local_flake_reference_path(reference: &str) -> Option<PathBuf> {
  // Only preflight references that are unmistakably filesystem paths. Bare
  // names like `nixpkgs`, plus URL/registry-style refs, stay in Nix's hands.
  if let Some(path) = reference.strip_prefix("path:") {
    // Query parameters affect Nix's flakeref, not the local path existence
    // check. Keep the original reference unchanged for command execution.
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    return Some(PathBuf::from(path));
  }

  let path = Path::new(reference);

  if path.is_absolute()
    || matches!(path.to_str(), Some("." | ".."))
    || path.starts_with("./")
    || path.starts_with("../")
  {
    return Some(path.to_path_buf());
  }

  None
}

#[test]
fn test_join_attribute() {
  assert_eq!(join_attribute(vec!["foo", "bar"]), "foo.bar");
  assert_eq!(join_attribute(vec!["foo", "bar.baz"]), r#"foo."bar.baz""#);
  assert_eq!(
    join_attribute(vec!["foo", r#"bar"baz"#, r"bar\baz", ""]),
    "foo.\"bar\\\"baz\".\"bar\\\\baz\".\"\""
  );
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
    }
  }
}

/// Attempts to find a default installable for `NixOS` builds.
///
/// Checks if `/etc/nixos/flake.nix` exists and returns a flake installable
/// pointing to it if found. If the directory is a symlink, it is resolved to
/// its canonical path. Otherwise, returns an error with instructions on how to
/// specify an installable.
///
/// # Errors
///
/// Returns an error if:
///
/// - No flake is found at `/etc/nixos/flake.nix`
/// - Permission is denied accessing the path
/// - The resolved path contains invalid UTF-8
fn try_find_default_for_os() -> color_eyre::Result<Installable> {
  use tracing::warn;

  let default_dir = std::path::Path::new("/etc/nixos");

  match resolve_fallback_flake_dir(default_dir) {
    Ok(resolved) => {
      warn!(
        "No installable was specified, falling back to {}",
        resolved.display()
      );
      Ok(Installable::Flake {
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
        "Permission denied accessing {}.\nPlease either:\n- Pass a flake path \
         as an argument (e.g., 'nh os switch .')\n- Set the NH_FLAKE \
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
        "No installable specified and no flake found at {}/flake.nix.\nPlease \
         either:\n- Pass a flake path as an argument (e.g., 'nh os switch \
         .')\n- Set the NH_FLAKE environment variable\n- Set the NH_OS_FLAKE \
         environment variable\n\n{}",
        default_dir.display(),
        FALLBACK_HELP_HINT
      ))
    },
  }
}

/// Attempts to find a default installable for Home Manager builds.
///
/// Checks if `$HOME/.config/home-manager/flake.nix` exists and returns a flake
/// installable pointing to it if found. If the directory is a symlink, it is
/// resolved to its canonical path. Otherwise, returns an error with
/// instructions on how to specify an installable.
///
/// # Errors
///
/// Returns an error if:
///
/// - The `HOME` environment variable is not set
/// - No flake is found at `$HOME/.config/home-manager/flake.nix`
/// - Permission is denied accessing the path
/// - The resolved path contains invalid UTF-8
fn try_find_default_for_home() -> color_eyre::Result<Installable> {
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
      Ok(Installable::Flake {
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
        "Permission denied accessing {}.\nPlease either:\n- Pass a flake path \
         as an argument (e.g., 'nh home switch .')\n- Set the NH_FLAKE \
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
        "No installable specified and no flake found at {}/flake.nix.\nPlease \
         either:\n- Pass a flake path as an argument (e.g., 'nh home switch \
         .')\n- Set the NH_FLAKE environment variable\n- Set the \
         NH_HOME_FLAKE environment variable\n\n{}",
        default_dir.display(),
        FALLBACK_HELP_HINT
      ))
    },
  }
}

/// Attempts to find a default installable for Darwin builds.
///
/// Checks if `/etc/nix-darwin/flake.nix` exists and returns a flake installable
/// pointing to it if found. If the directory is a symlink, it is resolved to
/// its canonical path. Otherwise, returns an error with instructions on how to
/// specify an installable.
///
/// # Errors
///
/// Returns an error if:
///
/// - No flake is found at `/etc/nix-darwin/flake.nix`
/// - Permission is denied accessing the path
/// - The resolved path contains invalid UTF-8
fn try_find_default_for_darwin() -> color_eyre::Result<Installable> {
  use tracing::warn;

  let default_dir = std::path::Path::new("/etc/nix-darwin");

  match resolve_fallback_flake_dir(default_dir) {
    Ok(resolved) => {
      warn!(
        "No installable was specified, falling back to {}",
        resolved.display()
      );
      Ok(Installable::Flake {
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
        "Permission denied accessing {}.\nPlease either:\n- Pass a flake path \
         as an argument (e.g., 'nh darwin switch .')\n- Set the NH_FLAKE \
         environment variable\n- Set the NH_DARWIN_FLAKE environment \
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
        "No installable specified and no flake found at {}/flake.nix.\nPlease \
         either:\n- Pass a flake path as an argument (e.g., 'nh darwin switch \
         .')\n- Set the NH_FLAKE environment variable\n- Set the \
         NH_DARWIN_FLAKE environment variable\n\n{}",
        default_dir.display(),
        FALLBACK_HELP_HINT
      ))
    },
  }
}

#[cfg(test)]
#[expect(clippy::panic, clippy::unwrap_used, reason = "Fine in tests")]
mod tests;

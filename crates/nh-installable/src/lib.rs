use std::{
  env,
  fs,
  path::{Path, PathBuf},
};

use misstep::OptionExt;
pub use nix_installable::Installable;
use nix_installable::{parse_attribute, parse_flake_reference};
use pound::{ArgSpec, CommandSpec, ErrorKind, Hint, Kind, Matches, Parse};
use tracing::debug;

mod defaults;

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

  fn into_installable(self) -> misstep::Result<Installable> {
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
            .map_err(|err| misstep::report!("NH_ATTRP {err}"))?,
        })
      },
      Self::GenericFlake(value) => {
        debug!("Using NH_FLAKE: {value}");
        flake_from_env_var("NH_FLAKE", &value)
      },
    }
  }
}

impl Parse for InstallableArgs {
  const SPEC: &'static CommandSpec = &CommandSpec::new("installable")
    .args(&[
      ArgSpec::new(Kind::Opt).short('f').long("file").hidden(),
      ArgSpec::new(Kind::Opt).short('E').long("expr").hidden(),
      ArgSpec::new(Kind::Positional)
        .hint(Hint::Path)
        .value_name("INSTALLABLE")
        .help("Which installable to use")
        .long_help(
          r"Which installable to use.
Nix accepts various kinds of installables:

[FLAKEREF[#ATTRPATH]]
    Flake reference with an optional attribute path.
    [env: NH_FLAKE, NH_OS_FLAKE, NH_HOME_FLAKE, NH_DARWIN_FLAKE]

-f, --file <FILE> [ATTRPATH]
    Path to file with an optional attribute path.
    [env: NH_FILE, NH_ATTRP]

-E, --expr <EXPR> [ATTRPATH]
    Nix expression with an optional attribute path.

[PATH]
    Path or symlink to a /nix/store path",
        ),
    ])
    .conflicts(&[(0, 1)]);

  fn from_matches(
    _spec: &'static CommandSpec,
    matches: &Matches<'_>,
  ) -> Result<Self, pound::Error> {
    let file = matches.raw(0);
    let expr = matches.raw(1);
    let installable = matches.raw(2);

    if let Some(i) = installable
      && let Ok(path) = fs::canonicalize(i)
      && path.starts_with("/nix/store")
    {
      return Ok(Self::Specified(Installable::Store { path }));
    }

    let invalid = |value: &str, msg: String| {
      pound::Error::from(ErrorKind::Value {
        arg: "INSTALLABLE".to_owned(),
        value: value.to_owned(),
        msg,
      })
    };
    if let Some(path) = file {
      let raw = installable.unwrap_or("");
      let attribute = parse_attribute(raw)
        .map_err(|err| invalid(raw, format!("attribute path {err}")))?;
      return Ok(Self::Specified(Installable::File {
        path: PathBuf::from(path),
        attribute,
      }));
    }
    if let Some(expression) = expr {
      let raw = installable.unwrap_or("");
      let attribute = parse_attribute(raw)
        .map_err(|err| invalid(raw, format!("attribute path {err}")))?;
      return Ok(Self::Specified(Installable::Expression {
        expression: expression.to_owned(),
        attribute,
      }));
    }
    if let Some(raw) = installable {
      let (reference, attribute) = parse_flake_reference(raw)
        .map_err(|err| invalid(raw, format!("installable argument {err}")))?;
      return Ok(Self::Specified(Installable::Flake {
        reference,
        attribute,
      }));
    }
    Ok(Self::Unspecified)
  }

  fn update_from_matches(
    &mut self,
    spec: &'static CommandSpec,
    matches: &Matches<'_>,
  ) -> Result<(), pound::Error> {
    if (0..3).any(|index| matches.given(index)) {
      *self = Self::from_matches(spec, matches)?;
    }
    Ok(())
  }
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
  ) -> misstep::Result<Option<Installable>> {
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
  ) -> misstep::Result<Installable> {
    let Some(mut installable) = self.resolve(context)? else {
      return default_installable_for(context);
    };

    if let Installable::File { path, .. } = &mut installable {
      *path = defaults::resolve_file(context, std::mem::take(path));
    }
    validate_local_flake_ref(&installable, context)?;
    Ok(installable)
  }
}

/// Describes where a platform's configurations live in a flake and how to reach
/// a configuration's build output.
#[derive(Clone, Copy, Debug)]
pub struct ConfigurationLayout<'a> {
  /// Flake output set holding the configurations, e.g.
  /// `"nixosConfigurations"`.
  pub set: &'a str,

  /// Attribute path appended after the configuration name to reach the build
  /// output, e.g. `["config", "system", "build", "toplevel"]`.
  pub build_attr: &'a [&'a str],
}

/// Resolution of a flake output set (`nixosConfigurations`,
/// `homeConfigurations`, `darwinConfigurations`) down to a single
/// configuration's build output.
pub trait ConfigurationInstallable {
  /// Rewrites the installable to point at one configuration's build output as
  /// described by `layout`.
  ///
  /// For a flake, the attribute is normalized to `<set>.<name>` followed by
  /// `layout.build_attr`. The configuration name is taken from the attribute
  /// when present (`.#name` or `.#<set>.name`), otherwise `inferred_name` is
  /// used. File and expression installables only gain the build attribute, and
  /// a store path is left untouched.
  ///
  /// # Errors
  ///
  /// Returns an error when the flake attribute is more specific than a single
  /// configuration name, or when it names no configuration and `inferred_name`
  /// is `None`.
  fn resolve_configuration(
    &mut self,
    layout: ConfigurationLayout<'_>,
    inferred_name: Option<&str>,
  ) -> misstep::Result<()>;
}

impl ConfigurationInstallable for Installable {
  fn resolve_configuration(
    &mut self,
    layout: ConfigurationLayout<'_>,
    inferred_name: Option<&str>,
  ) -> misstep::Result<()> {
    let build_attr = || layout.build_attr.iter().copied().map(String::from);

    match self {
      Self::Flake { attribute, .. } => {
        let mut resolved =
          configuration_selector(attribute, layout.set, inferred_name)?;
        resolved.extend(build_attr());
        *attribute = resolved;
      },
      Self::File { attribute, .. } | Self::Expression { attribute, .. } => {
        attribute.extend(build_attr());
      },
      Self::Store { .. } => {},
    }

    Ok(())
  }
}

/// Reduces a flake attribute to the `<set>.<name>` selector for a single
/// configuration, taking the name from the attribute or `inferred_name`.
fn configuration_selector(
  attribute: &[String],
  set: &str,
  inferred_name: Option<&str>,
) -> misstep::Result<Vec<String>> {
  let name_part = match attribute.split_first() {
    Some((first, rest)) if first.as_str() == set => rest,
    _ => attribute,
  };

  let name = match name_part {
    [name] => name.clone(),
    [] => {
      inferred_name
        .context("No configuration name given. Specify one, e.g. '.#name'.")?
        .to_owned()
    },
    [first, ..] => {
      return Err(misstep::report!(
        "Attribute path is too specific: {}. Specify only the configuration \
         name, e.g. '.#{first}'.",
        attribute.join(".")
      ));
    },
  };

  Ok(vec![set.to_owned(), name])
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
) -> misstep::Result<Installable> {
  defaults::installable_for(context)
}

fn flake_from_env_var(name: &str, value: &str) -> misstep::Result<Installable> {
  let (reference, attribute) = parse_flake_reference(value)
    .map_err(|err| misstep::report!("{name} {err}"))?;
  Ok(Installable::Flake {
    reference,
    attribute,
  })
}

/// Validates an explicit local flake reference before Nix is invoked.
///
/// Nix would otherwise search parent directories for `flake.nix`. Failing here
/// instead points the error at the reference the user supplied.
///
/// # Errors
///
/// Returns an error when the reference names a local path that does not exist
/// or does not contain a `flake.nix`.
fn validate_local_flake_ref(
  installable: &Installable,
  context: CommandContext,
) -> misstep::Result<()> {
  let Installable::Flake { reference, .. } = installable else {
    return Ok(());
  };

  let Some(path) = local_flake_reference_path(reference) else {
    return Ok(());
  };

  defaults::validate_flake_reference(
    reference,
    &path,
    context.specific_flake_env_var(),
  )
}

/// Only preflight references that are unmistakably filesystem paths.
fn local_flake_reference_path(reference: &str) -> Option<PathBuf> {
  // Only preflight references that are unmistakably filesystem paths. Bare
  // names like `nixpkgs`, plus URL/registry-style refs, stay in Nix's hands.
  // Parameterized local flake references such as `path:/repo?dir=nix/flakes`
  // and `./nix?submodules=1` have scheme- and repository-dependent semantics,
  // so we leave their interpretation to Nix as well.
  if reference.contains('?') {
    return None;
  }

  if let Some(path) = reference.strip_prefix("path:") {
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

#[cfg(test)]
#[expect(clippy::panic, clippy::unwrap_used, reason = "Fine in tests")]
mod tests;

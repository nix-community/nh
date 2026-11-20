use std::{fs, path::PathBuf};

use nh_installable::Installable;
use nix_installable::{parse_attribute, parse_flake_reference};
use pound::{ArgSpec, CommandSpec, ErrorKind, Hint, Kind, Matches, Parse};

/// Which installable to use, given as a positional, `--file` or `--expr`.
#[derive(Debug, Clone)]
pub struct InstallableArgs(pub nh_installable::InstallableArgs);

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
      return Ok(Self(nh_installable::InstallableArgs::Specified(
        Installable::Store { path },
      )));
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
      return Ok(Self(nh_installable::InstallableArgs::Specified(
        Installable::File {
          path: PathBuf::from(path),
          attribute,
        },
      )));
    }
    if let Some(expression) = expr {
      let raw = installable.unwrap_or("");
      let attribute = parse_attribute(raw)
        .map_err(|err| invalid(raw, format!("attribute path {err}")))?;
      return Ok(Self(nh_installable::InstallableArgs::Specified(
        Installable::Expression {
          expression: expression.to_owned(),
          attribute,
        },
      )));
    }
    if let Some(raw) = installable {
      let (reference, attribute) = parse_flake_reference(raw)
        .map_err(|err| invalid(raw, format!("installable argument {err}")))?;
      return Ok(Self(nh_installable::InstallableArgs::Specified(
        Installable::Flake {
          reference,
          attribute,
        },
      )));
    }
    Ok(Self(nh_installable::InstallableArgs::Unspecified))
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

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "Fine in tests")]
mod tests {
  use pound::Parse;

  use super::InstallableArgs;

  #[test]
  fn test_cli_installable_rejects_empty_flake_reference() {
    let err = InstallableArgs::try_parse_from([""])
      .unwrap_err()
      .to_string();

    assert!(err.contains("installable argument is empty"));
  }

  #[test]
  fn test_cli_installable_rejects_attribute_without_reference() {
    let err = InstallableArgs::try_parse_from(["#fallback"])
      .unwrap_err()
      .to_string();

    assert!(
      err.contains("installable argument missing reference part before `#`")
    );
  }

  #[test]
  fn test_cli_file_rejects_malformed_attribute() {
    let err =
      InstallableArgs::try_parse_from(["--file", "file.nix", r#"foo."bar"#])
        .unwrap_err()
        .to_string();

    assert!(
      err.contains("attribute path contains an unclosed quoted attribute")
    );
  }
}

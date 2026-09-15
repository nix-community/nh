use std::str::FromStr;

use color_eyre::eyre::{Result, bail};
use nh_core::command::Build;
use nh_installable::Installable;
use tracing::info;

pub const ENVIRONMENT_VARIABLE: &str = "NIXOS_LABEL";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenerationLabel {
  value:          String,
  was_normalized: bool,
}

impl GenerationLabel {
  pub(crate) fn as_str(&self) -> &str {
    &self.value
  }

  pub(crate) fn report_normalization(&self) {
    if self.was_normalized {
      info!("Normalized NixOS label to: {}", self.value);
    }
  }

  pub(crate) fn ensure_evaluable(
    &self,
    installable: &Installable,
  ) -> Result<()> {
    if let Installable::Store { path } = installable {
      bail!(
        "cannot apply --label '{}' to existing store-path installable '{}' \
         because it has already been evaluated",
        self.as_str(),
        path.display()
      );
    }

    Ok(())
  }

  pub(crate) fn configure_build(&self, build: Build) -> Build {
    build.env(ENVIRONMENT_VARIABLE, self.as_str()).impure(true)
  }

  pub(crate) fn environment(&self) -> (&'static str, &str) {
    (ENVIRONMENT_VARIABLE, self.as_str())
  }
}

impl FromStr for GenerationLabel {
  type Err = String;

  fn from_str(raw: &str) -> Result<Self, Self::Err> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
      return Err("label must not be empty or contain only whitespace".into());
    }

    let mut value = String::with_capacity(trimmed.len());
    let mut whitespace = false;

    for character in trimmed.chars() {
      if character.is_whitespace() {
        whitespace = true;
        continue;
      }

      if whitespace {
        value.push('-');
        whitespace = false;
      }

      if character.is_ascii_alphanumeric()
        || matches!(character, ':' | '_' | '.' | '-')
      {
        value.push(character);
      } else {
        return Err(format!(
          "label contains unsupported character {character:?}; only letters, \
           numbers, whitespace, and `:`, `_`, `.`, or `-` are allowed"
        ));
      }
    }

    Ok(Self {
      was_normalized: value != raw,
      value,
    })
  }
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use nh_core::command::Build;
  use nh_installable::Installable;

  use super::{ENVIRONMENT_VARIABLE, GenerationLabel};

  #[test]
  fn normalizes_label_whitespace() {
    let result = "\u{2003}try   new\tdriver\u{2003}".parse::<GenerationLabel>();
    assert_eq!(
      result,
      Ok(GenerationLabel {
        value:          "try-new-driver".into(),
        was_normalized: true,
      })
    );
  }

  #[test]
  fn accepts_nixos_label_characters() {
    let result = "NixOS:26.11_test-1".parse::<GenerationLabel>();
    assert_eq!(
      result,
      Ok(GenerationLabel {
        value:          "NixOS:26.11_test-1".into(),
        was_normalized: false,
      })
    );
  }

  #[test]
  fn rejects_empty_and_invalid_labels() {
    assert!(" \t".parse::<GenerationLabel>().is_err());
    assert!("new/driver".parse::<GenerationLabel>().is_err());
  }

  #[test]
  fn rejects_already_evaluated_installables() {
    let label = GenerationLabel {
      value:          "test-label".into(),
      was_normalized: false,
    };
    let installable = Installable::Store {
      path: PathBuf::from("/nix/store/example-system"),
    };

    assert!(label.ensure_evaluable(&installable).is_err());
  }

  #[test]
  fn configures_nix_build_with_native_label_environment() {
    let label = GenerationLabel {
      value:          "test-label".into(),
      was_normalized: false,
    };
    let installable = Installable::Flake {
      reference: ".".into(),
      attribute: vec!["nixosConfigurations".into(), "host".into()],
    };

    let command = label
      .configure_build(Build::new(installable))
      .to_nix_command();
    let (_, args, env) = command.into_parts();

    assert!(args.contains(&"--impure".into()));
    assert_eq!(env, vec![(
      ENVIRONMENT_VARIABLE.into(),
      "test-label".into()
    )]);
  }
}

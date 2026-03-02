use clap::Args;
use color_eyre::Result;
use tracing::warn;

use crate::{command::Command, installable::Installable};

#[derive(Debug, Args)]
pub struct UpdateArgs {
  #[arg(short = 'u', long = "update", conflicts_with = "update_input")]
  /// Update all flake inputs
  pub update_all: bool,

  #[arg(short = 'U', long = "update-input", conflicts_with = "update_all")]
  /// Update the specified flake input(s)
  pub update_input: Option<Vec<String>>,
}

pub fn update(
  installable: &Installable,
  inputs: Option<Vec<String>>,
  commit_lock_file: bool,
) -> Result<()> {
  let Installable::Flake { reference, .. } = installable else {
    warn!(
      "Only flake installables can be updated, {} is not supported",
      installable.str_kind()
    );
    return Ok(());
  };

  let mut cmd = Command::new("nix").args(["flake", "update"]);

  if commit_lock_file {
    cmd = cmd.arg("--commit-lock-file");
  }

  if let Some(inputs) = inputs {
    for input in &inputs {
      cmd = cmd.arg(input);
    }
    cmd = cmd.message(format!(
      "Updating flake input{maybe_plural} {inputs}",
      maybe_plural = if inputs.len() > 1 { "s" } else { "" },
      inputs = inputs.join(", ")
    ));
  } else {
    cmd = cmd.message("Updating all flake inputs");
  }

  cmd.arg("--flake").arg(reference).run()?;

  Ok(())
}

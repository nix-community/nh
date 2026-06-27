use clap::Args;
use color_eyre::{Result, eyre::bail};
use nh_installable::Installable;
use nix_command::{CommandKind, NixCommand};
use tracing::{info, warn};

#[derive(Debug, Args)]
pub struct UpdateArgs {
  #[arg(short = 'u', long = "update", conflicts_with = "update_input")]
  /// Update all flake inputs
  pub update_all: bool,

  #[arg(short = 'U', long = "update-input", conflicts_with = "update_all")]
  /// Update the specified flake input(s)
  pub update_input: Option<Vec<String>>,
}

/// Update flake inputs for an installable.
///
/// # Errors
///
/// Returns an error if `nix flake update` fails.
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

  let mut cmd = NixCommand::new(CommandKind::Flake).arg("update");

  if commit_lock_file {
    cmd = cmd.arg("--commit-lock-file");
  }

  let message = match inputs {
    Some(inputs) if !inputs.is_empty() => {
      cmd = cmd.args(&inputs);

      let maybe_plural = if inputs.len() > 1 { "s" } else { "" };
      format!("Updating flake input{maybe_plural} {}", inputs.join(", "))
    },
    _ => "Updating all flake inputs".to_string(),
  };

  info!("{message}");

  let status = cmd.arg("--flake").arg(reference).run_with_logs()?;

  if !status.success() {
    bail!("{message} (exit status {status:?})");
  }

  Ok(())
}

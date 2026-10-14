use std::{
  env,
  ffi::{OsStr, OsString},
  path::PathBuf,
};

use color_eyre::eyre::{Context, ContextCompat, Result};
use serde::{Deserialize, Serialize};
use tracing::level_filters::LevelFilter;

use crate::command::{Command, ElevationStrategy};

const PRIVILEGED_SUBCOMMAND: &str = "__privileged";

#[derive(Debug, Serialize, Deserialize)]
pub struct PrivilegedRequest {
  pub log_level: String,
  pub op:        PrivilegedOp,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum PrivilegedOp {
  NixosActivate(NixosActivation),
  NixosRollback(NixosRollback),
  DarwinActivate(DarwinActivation),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum NixosActivationAction {
  Switch,
  Boot,
  Test,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NixosActivation {
  pub action:                         NixosActivationAction,
  pub switch_to_configuration:        PathBuf,
  pub system:                         PathBuf,
  pub continue_on_activation_failure: bool,
  pub install_bootloader:             bool,
  pub show_activation_logs:           bool,
  pub nix_args:                       Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NixosRollback {
  pub target_profile:          PathBuf,
  pub previous_profile:        Option<PathBuf>,
  pub switch_to_configuration: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DarwinActivation {
  pub system:               PathBuf,
  pub activate:             bool,
  pub show_activation_logs: bool,
  pub nix_args:             Vec<String>,
}

impl PrivilegedOp {
  /// Runs this operation in one elevated `nh` process.
  ///
  /// # Errors
  ///
  /// Returns an error if the request cannot be encoded or the elevated process
  /// fails.
  pub fn run_elevated(self, strategy: ElevationStrategy) -> Result<()> {
    let request = serde_json::to_string(&PrivilegedRequest {
      log_level: LevelFilter::current().to_string(),
      op:        self,
    })?;
    Command::new(
      env::current_exe().context("Failed to get current executable path")?,
    )
    .args([PRIVILEGED_SUBCOMMAND, &request])
    .elevate(Some(strategy))
    .preserve_envs(["NIXOS_INSTALL_BOOTLOADER", "NIXOS_NO_CHECK"])
    .with_required_env()
    .show_output(true)
    .run()
  }
}

impl PrivilegedRequest {
  /// Extracts the request when `nh` was started as an elevated worker.
  ///
  /// # Errors
  ///
  /// Returns an error if the request is missing or cannot be decoded.
  pub fn from_args(
    mut args: impl Iterator<Item = OsString>,
  ) -> Option<Result<Self>> {
    (args.next()? == OsStr::new(PRIVILEGED_SUBCOMMAND)).then(|| {
      let request = args.next().context("Missing privileged request")?;
      let request = request
        .to_str()
        .context("Privileged request is not valid UTF-8")?;
      serde_json::from_str(request).context("Invalid privileged request")
    })
  }

  /// # Errors
  ///
  /// Returns an error if the log level cannot be parsed.
  pub fn log_level(&self) -> Result<LevelFilter> {
    self
      .log_level
      .parse()
      .context("Invalid log level in privileged request")
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn request_round_trips_through_args() {
    let request = serde_json::to_string(&PrivilegedRequest {
      log_level: LevelFilter::DEBUG.to_string(),
      op:        PrivilegedOp::NixosRollback(NixosRollback {
        target_profile:          PathBuf::from(
          "/nix/var/nix/profiles/system-2-link",
        ),
        previous_profile:        None,
        switch_to_configuration: PathBuf::from("/bin/switch-to-configuration"),
      }),
    })
    .unwrap();
    let args = [
      OsString::from(PRIVILEGED_SUBCOMMAND),
      OsString::from(request),
    ];
    let parsed = PrivilegedRequest::from_args(args.into_iter())
      .unwrap()
      .unwrap();

    assert_eq!(parsed.log_level().unwrap(), LevelFilter::DEBUG);
    assert!(matches!(
      parsed.op,
      PrivilegedOp::NixosRollback(NixosRollback {
        previous_profile: None,
        ..
      })
    ));
    assert!(
      PrivilegedRequest::from_args([OsString::from("os")].into_iter())
        .is_none()
    );
  }
}

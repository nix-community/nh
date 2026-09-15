use std::{fs, path::Path};

use color_eyre::{
  Result,
  eyre::{Context, bail, eyre},
};
use nh_core::{
  args::DiffType,
  command::{Command, ElevationStrategy},
};
use nh_diff::print_dix_diff;
use tracing::{info, warn};

use crate::{
  CURRENT_PROFILE,
  SYSTEM_PROFILE,
  activation_command,
  args::DarwinRollbackArgs,
  elevation_if_needed,
};

impl DarwinRollbackArgs {
  pub(super) fn rollback(self, elevation: ElevationStrategy) -> Result<()> {
    if nix::unistd::Uid::effective().is_root() && !self.bypass_root_check {
      bail!(
        "Don't run nh darwin as root. I will call sudo internally as needed"
      );
    }
    self.rollback_with(
      Path::new(SYSTEM_PROFILE),
      Path::new(CURRENT_PROFILE),
      elevation_if_needed(elevation).as_ref(),
      switch_generation,
      |activation| activation.run(),
    )
  }

  // Keep system paths and side effects at the boundary so the rollback flow
  // can also be exercised against isolated profiles.
  fn rollback_with(
    self,
    profile: &Path,
    current_profile: &Path,
    elevation: Option<&ElevationStrategy>,
    mut select: impl FnMut(&Path, u64, Option<&ElevationStrategy>) -> Result<()>,
    activate: impl FnOnce(Command) -> Result<()>,
  ) -> Result<()> {
    let original = current_generation(profile)?;
    let target = match self.to {
      Some(number) => number,
      None => previous_generation(profile, original)?,
    };
    let generation_link =
      profile.with_file_name(format!("system-{target}-link"));
    let system = generation_link.canonicalize().with_context(|| {
      format!("Darwin generation {target} does not exist or is unavailable")
    })?;
    let activation = activation_command(
      &system,
      elevation.cloned(),
      self.show_activation_logs,
    )?;

    info!("Rolling back to Darwin generation {target}");
    if let (Ok(selected), Ok(active)) =
      (profile.canonicalize(), current_profile.canonicalize())
      && selected != active
    {
      warn!(
        "System profile differs from the running configuration; selecting \
         generations relative to profile generation {original}"
      );
    }

    if !matches!(self.diff, DiffType::Never)
      && let Err(error) = print_dix_diff(current_profile, &system)
    {
      if matches!(self.diff, DiffType::Always) {
        return Err(error).wrap_err("Failed to show Darwin rollback diff");
      }
      warn!("Could not show Darwin rollback diff: {error:#}");
    }

    if self.dry {
      info!(
        "Dry run: would select generation {target} and activate {}",
        system.display()
      );
      return Ok(());
    }

    if self.ask
      && !inquire::Confirm::new(&format!(
        "Roll back to Darwin generation {target}?"
      ))
      .with_default(false)
      .prompt()?
    {
      bail!("User rejected the rollback");
    }

    // Confirmation and diffing may take time. Do not apply a stale selection.
    ensure_current_generation(profile, original)?;
    if generation_link
      .canonicalize()
      .context("Rollback target is no longer available")?
      != system
    {
      bail!("Darwin rollback target changed; retry the rollback");
    }

    select(profile, target, elevation)
      .wrap_err("Failed to set Darwin system profile during rollback")?;
    ensure_current_generation(profile, target)?;

    if let Err(error) = activate(activation) {
      // Restoring the profile cannot undo effects of a partially run
      // activation.
      let recovery = (|| -> Result<()> {
        ensure_current_generation(profile, target)?;
        if profile.canonicalize()? != system {
          bail!("System profile target changed; refusing to overwrite it");
        }
        if original != target {
          select(profile, original, elevation)?;
        }
        Ok(())
      })();

      return match recovery {
        Ok(()) => {
          Err(error).wrap_err(format!(
            "Darwin activation failed; system profile restored to generation \
             {original}. Activation may have partially changed the system"
          ))
        },
        Err(recovery_error) => {
          Err(error).wrap_err(format!(
            "Darwin activation failed and the original system profile could \
             not be restored: {recovery_error:#}. Activation may have \
             partially changed the system"
          ))
        },
      };
    }

    info!("Successfully rolled back to Darwin generation {target}");
    Ok(())
  }
}

#[cfg(test)]
#[path = "rollback_tests.rs"]
mod tests;

/// Read the generation number from the profile link, not its store path.
fn current_generation(profile: &Path) -> Result<u64> {
  let link = fs::read_link(profile).with_context(|| {
    format!("Failed to read Darwin system profile {}", profile.display())
  })?;
  generation_number(&link).ok_or_else(|| {
    eyre!(
      "Darwin system profile does not point to a system generation: {}",
      link.display()
    )
  })
}

fn generation_number(link: &Path) -> Option<u64> {
  link
    .file_name()?
    .to_str()?
    .strip_prefix("system-")?
    .strip_suffix("-link")?
    .parse()
    .ok()
}

/// Choose the nearest retained generation below the selected profile.
fn previous_generation(profile: &Path, current: u64) -> Result<u64> {
  let directory = profile
    .parent()
    .ok_or_else(|| eyre!("System profile has no parent directory"))?;
  let mut previous = None;
  for entry in
    fs::read_dir(directory).context("Failed to list Darwin generations")?
  {
    let entry = entry.context("Failed to read Darwin generation entry")?;
    if let Some(number) = generation_number(&entry.path())
      && number < current
      && entry.file_type()?.is_symlink()
    {
      previous = Some(previous.map_or(number, |other: u64| other.max(number)));
    }
  }
  previous
    .ok_or_else(|| eyre!("No Darwin generation older than {current} exists"))
}

fn ensure_current_generation(profile: &Path, expected: u64) -> Result<()> {
  if current_generation(profile)? != expected {
    bail!(
      "Darwin system profile changed during rollback; refusing to overwrite it"
    );
  }
  Ok(())
}

fn switch_generation(
  profile: &Path,
  number: u64,
  elevation: Option<&ElevationStrategy>,
) -> Result<()> {
  Command::new("nix-env")
    .arg("--profile")
    .arg(profile)
    .arg("--switch-generation")
    .arg(number.to_string())
    .elevate(elevation.cloned())
    .message(format!("Selecting Darwin generation {number}"))
    .with_required_env()
    .run()
}

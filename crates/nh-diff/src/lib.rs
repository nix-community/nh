use std::{
  io,
  path::{Path, PathBuf},
};

use misstep::Result;
use nh_core::{
  args::{DiffArgs, DiffType},
  progress,
};
use nh_remote::{RemoteHost, query_remote_snapshot, resolve_remote_store_path};
use tracing::{debug, info, warn};
use yansi::Paint;

const NIXOS_CURRENT_PROFILE: &str = "/run/current-system";

/// Prints the difference between two generations in terms of paths and closure
/// sizes. `diff.mode` is up to the caller.
///
/// # Returns
///
/// `Ok(())` once the diff is printed.
///
/// # Errors
///
/// Returns an error if querying the store or writing the diff report fails.
pub fn print_dix_diff(
  old_generation: &Path,
  new_generation: &Path,
  diff: &DiffArgs,
) -> Result<()> {
  let report = dix::query_diff_report(old_generation, new_generation)?;
  print_report(
    &display_path(old_generation),
    &display_path(new_generation),
    &report,
    diff,
  )
}

/// Handles NixOS system diffing for local and remote rebuilds.
///
/// `target_profile` must be in the store of `target_host`, or the local store
/// if there is none.
///
/// Remote diffs require dix on the target host unless both closures are
/// already in the local store.
///
/// # Returns
///
/// `Ok(())` once the diff is printed, skipped, or failed with
/// [`DiffType::Auto`].
///
/// # Errors
///
/// Returns an error if the diff fails and was requested with
/// [`DiffType::Always`].
pub fn handle_nixos_diff(
  diff: &DiffArgs,
  target_host: Option<&RemoteHost>,
  target_profile: &Path,
) -> Result<()> {
  let current_profile = Path::new(NIXOS_CURRENT_PROFILE);

  if matches!(diff.mode, DiffType::Never) {
    debug!("Not running dix as the --diff flag is set to never.");
    return Ok(());
  }

  let result = target_host.map_or_else(
    || print_dix_diff(current_profile, target_profile, diff),
    |target_host| {
      print_remote_nixos_diff(
        target_host,
        current_profile,
        target_profile,
        diff,
      )
    },
  );
  tolerate_diff_failure(&diff.mode, result)
}

/// The diff is informational, so a failing diff only aborts the rebuild if it
/// was explicitly requested.
///
/// # Returns
///
/// `result`, except that failures with [`DiffType::Auto`] are logged as a
/// warning and turned into `Ok(())`.
///
/// # Errors
///
/// Returns the error in `result` unless `diff` is [`DiffType::Auto`].
fn tolerate_diff_failure(diff: &DiffType, result: Result<()>) -> Result<()> {
  match (diff, result) {
    (DiffType::Auto, Err(err)) => {
      warn!("Skipping diff: {err:#}");
      Ok(())
    },
    (_, result) => result,
  }
}

/// Diffs the target host's current system against the new one.
///
/// A store path determines its closure, so each path is snapshotted from the
/// local store if possible. Only paths missing locally need dix on the target
/// host.
///
/// # Returns
///
/// `Ok(())` once the diff is printed.
///
/// # Errors
///
/// Returns an error if a path cannot be resolved on the target host, a
/// snapshot can be taken neither locally nor with dix on the target host, or
/// the report cannot be written.
fn print_remote_nixos_diff(
  target_host: &RemoteHost,
  current_profile: &Path,
  target_profile: &Path,
  diff: &DiffArgs,
) -> Result<()> {
  let snapshot = |path: &Path| {
    dix::query_store_snapshot(path)
      .or_else(|_| query_remote_snapshot(target_host, path))
  };

  let spinner = progress::spinner(format!(
    "Gathering system diff data for remote host '{target_host}'..."
  ));
  let old = resolve_remote_store_path(target_host, current_profile)?;
  let new = resolve_remote_store_path(target_host, target_profile)?;
  let report = dix::diff_store_snapshots(&snapshot(&old)?, &snapshot(&new)?);
  drop(spinner);

  print_report(&old, &new, &report, diff)
}

fn display_path(path: &Path) -> PathBuf {
  std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Prints `report` with `<<<`/`>>>` headers naming the compared paths, and a
/// note on how to show the packages it hides.
///
/// # Returns
///
/// `Ok(())` once the report is printed.
///
/// # Errors
///
/// Returns an error if the report cannot be rendered.
fn print_report(
  old_label: &Path,
  new_label: &Path,
  report: &dix::DiffReport,
  diff: &DiffArgs,
) -> Result<()> {
  let options = dix::RenderOptions {
    full: diff.full,
    ..dix::RenderOptions::default()
  };
  let mut out = String::new();
  let hidden = dix::write_diff_report(&mut out, report, options)?;

  println!("{} {}", "<<<".bold(), old_label.display());
  println!("{} {}", ">>>".bold(), new_label.display());
  print!("{out}");
  dix::write_hidden_note(&mut io::stdout().lock(), hidden, "--diff-full")?;

  if report.diffs().len() == hidden && report.size_old() == report.size_new() {
    info!("No version or size changes.");
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use misstep::report;

  use super::*;

  #[test]
  fn diff_failure_only_warns_with_auto() {
    let failure = || Err(report!("dix not found"));

    assert!(tolerate_diff_failure(&DiffType::Auto, failure()).is_ok());
    assert!(tolerate_diff_failure(&DiffType::Always, failure()).is_err());
  }
}

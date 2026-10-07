use std::{fmt::Write as _, path::Path, thread};

use misstep::Result;
use nh_core::{args::DiffType, progress};
use nh_remote::{RemoteHost, query_remote_snapshot, resolve_remote_store_path};
use tracing::{debug, info, warn};
use yansi::Paint;

const NIXOS_CURRENT_PROFILE: &str = "/run/current-system";

/// Prints the difference between two generations in terms of paths and closure
/// sizes.
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
) -> Result<()> {
  print_diff(None, old_generation, new_generation)
}

/// Handles NixOS system diffing for local and remote rebuilds.
///
/// `target_profile` must be in the store of `target_host`, or the local store
/// if there is none.
///
/// Remote diffs require dix on the target host unless both closures are
/// already in the local store. With [`DiffType::Auto`], failing remote diffs
/// only warn.
///
/// # Returns
///
/// `Ok(())` once the diff is printed, skipped, or failed with
/// [`DiffType::Auto`].
///
/// # Errors
///
/// Returns an error if local or remote store snapshot queries fail, or if the
/// diff report cannot be written.
pub fn handle_nixos_diff(
  diff: &DiffType,
  target_host: Option<&RemoteHost>,
  target_profile: &Path,
) -> Result<()> {
  let current_profile = Path::new(NIXOS_CURRENT_PROFILE);

  if matches!(diff, DiffType::Never) {
    debug!("Not running dix as the --diff flag is set to never.");
    return Ok(());
  }

  let Some(target_host) = target_host else {
    if matches!(diff, DiffType::Auto) && !current_profile.exists() {
      warn!(
        "current profile {} does not exist, skipping dix diffing",
        current_profile.display()
      );
      return Ok(());
    }
    return print_dix_diff(current_profile, target_profile);
  };

  // The diff is informational, so a failing remote diff only aborts the
  // rebuild if it was explicitly requested.
  match (
    diff,
    print_diff(Some(target_host), current_profile, target_profile),
  ) {
    (DiffType::Auto, Err(err)) => {
      warn!("Skipping remote diff: {err:?}");
      Ok(())
    },
    (_, result) => result,
  }
}

/// Diffs `old` against `new`, both in the store of `host`, or the local store
/// if there is none.
///
/// # Returns
///
/// `Ok(())` once the diff is printed.
///
/// # Errors
///
/// Returns an error if either path cannot be snapshotted, or the report cannot
/// be written.
fn print_diff(host: Option<&RemoteHost>, old: &Path, new: &Path) -> Result<()> {
  let spinner = host.map(|host| {
    progress::spinner(format!(
      "Gathering system diff data for remote host '{host}'..."
    ))
  });
  let (old, new) = thread::scope(|scope| {
    let old = scope.spawn(|| snapshot(host, old));
    let new = snapshot(host, new);
    let old = old
      .join()
      .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    (old, new)
  });
  let (old, new) = (old?, new?);
  let report = dix::diff_store_snapshots(&old.snapshot, &new.snapshot);
  drop(spinner);

  print_report(&old.path, &new.path, &report)
}

/// Snapshots `path` in the store of `host`, or the local store if there is
/// none.
///
/// A store path determines its closure, so remote paths are snapshotted from
/// the local store if possible. Only paths missing locally need dix on `host`.
///
/// # Returns
///
/// A snapshot of the store path that `path` resolves to.
///
/// # Errors
///
/// Returns an error if `path` cannot be resolved, or snapshotted neither
/// locally nor with dix on `host`.
fn snapshot(
  host: Option<&RemoteHost>,
  path: &Path,
) -> Result<dix::SnapshotDocument> {
  let Some(host) = host else {
    return dix::Store::open().query_snapshot_document(path);
  };

  let path = resolve_remote_store_path(host, path)?;
  dix::Store::open()
    .query_snapshot_document(&path)
    .or_else(|err| {
      debug!(
        "Could not snapshot '{}' locally, using dix on '{host}': {err:#}",
        path.display()
      );
      query_remote_snapshot(host, &path)
    })
}

/// Prints `report` with `<<<`/`>>>` headers naming the compared paths.
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
) -> Result<()> {
  let mut out = String::new();
  writeln!(out, "{} {}", "<<<".bold(), old_label.display())?;
  writeln!(out, "{} {}", ">>>".bold(), new_label.display())?;
  let hidden =
    dix::write_diff_report(&mut out, report, dix::RenderOptions::default())?;
  print!("{out}");

  // Hidden packages only changed in amount or by a negligible size.
  let shows_changes =
    report.diffs.len() > hidden || report.size_old != report.size_new;
  if !shows_changes {
    info!("No version or size changes.");
  }

  Ok(())
}

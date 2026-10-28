use std::{
  fmt::Write as _,
  io::{self, Write as _},
  path::Path,
  thread,
};

use misstep::Result;
use nh_core::{
  args::{Diff, DiffType},
  progress,
};
use nh_remote::{RemoteHost, query_remote_snapshot, resolve_remote_store_path};
use tracing::{debug, info, warn};
use yansi::Paint;

/// Prints the difference between two generations in terms of paths and closure
/// sizes, as requested by `diff`.
///
/// Both generations must be in the store of `host`, or the local store if
/// there is none. Remote diffs require dix on `host` unless both closures are
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
pub fn print_diff(
  diff: &Diff,
  host: Option<&RemoteHost>,
  old: &Path,
  new: &Path,
) -> Result<()> {
  if matches!(diff.mode, DiffType::Never) {
    debug!("Not running dix as the --diff flag is set to never.");
    return Ok(());
  }

  debug!("Comparing {} with {}", old.display(), new.display());
  let options = dix::RenderOptions {
    full: diff.full,
    ..dix::RenderOptions::default()
  };
  // The diff is informational, so a failing diff only aborts the rebuild if it
  // was explicitly requested.
  match (&diff.mode, print_closure_diff(host, old, new, options)) {
    (DiffType::Auto, Err(err)) => {
      warn!("Skipping diff: {err:#}");
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
fn print_closure_diff(
  host: Option<&RemoteHost>,
  old: &Path,
  new: &Path,
  options: dix::RenderOptions,
) -> Result<()> {
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

  print_report(&old.path, &new.path, &report, options)
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
  options: dix::RenderOptions,
) -> Result<()> {
  let mut out = String::new();
  writeln!(out, "{} {}", "<<<".bold(), old_label.display())?;
  writeln!(out, "{} {}", ">>>".bold(), new_label.display())?;
  let hidden = dix::write_diff_report(&mut out, report, options)?;

  let mut stdout = io::stdout().lock();
  stdout.write_all(out.as_bytes())?;
  dix::write_hidden_note(&mut stdout, hidden, "--diff full")?;

  // Hidden packages only changed in amount or by a negligible size.
  let shows_changes =
    report.diffs.len() > hidden || report.size_old != report.size_new;
  if !shows_changes {
    info!("No version or size changes.");
  }

  Ok(())
}

use std::{
  fmt,
  io,
  path::{Path, PathBuf},
  thread,
};

use misstep::{Report, Result, report};
use nh_core::{args::DiffType, progress};
use nh_remote::{RemoteHost, ResolvedRemoteStorePath};
use tracing::{debug, info, warn};
use yansi::Paint;

const NIXOS_CURRENT_PROFILE: &str = "/run/current-system";

struct WriteFmt<W: io::Write>(W);
impl<W: io::Write> fmt::Write for WriteFmt<W> {
  fn write_str(&mut self, string: &str) -> fmt::Result {
    self.0.write_all(string.as_bytes()).map_err(|_| fmt::Error)
  }
}

struct QueriedDiff {
  old_label: PathBuf,
  new_label: PathBuf,
  report:    dix::DiffReport,
}

impl QueriedDiff {
  fn write(&self) -> Result<()> {
    print_dix_header_raw(&self.old_label, &self.new_label);
    write_dix_report(&self.report)
  }
}

/// Prints the difference between two generations in terms of paths and closure
/// sizes.
///
/// # Errors
///
/// Returns an error if querying the store or writing the diff report fails.
pub fn print_dix_diff(
  old_generation: &Path,
  new_generation: &Path,
) -> Result<()> {
  query_local_dix_diff(old_generation, new_generation)?.write()
}

fn query_local_dix_diff(
  old_generation: &Path,
  new_generation: &Path,
) -> Result<QueriedDiff> {
  let report = dix::query_diff_report(old_generation, new_generation, true)
    .map_err(|error| Report::from_boxed(error.into()))?;

  Ok(QueriedDiff {
    old_label: display_path(old_generation),
    new_label: display_path(new_generation),
    report,
  })
}

/// Handles NixOS system diffing for local and remote rebuilds.
///
/// `target_profile` must be in the store of `target_host`, or the local store
/// if there is none.
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

  match diff {
    DiffType::Never => {
      debug!("Not running dix as the --diff flag is set to never.");
      return Ok(());
    },
    DiffType::Auto if target_host.is_none() && !current_profile.exists() => {
      warn!(
        "current profile {} does not exist, skipping dix diffing",
        current_profile.display()
      );
      return Ok(());
    },
    DiffType::Auto => {
      debug!(
        "Comparing current profile {} with target profile: {}",
        current_profile.display(),
        target_profile.display()
      );
    },
    DiffType::Always => {},
  }

  let Some(target_host) = target_host else {
    return print_dix_diff(current_profile, target_profile);
  };

  let spinner = progress::spinner(format!(
    "Gathering system diff data from remote host '{target_host}'..."
  ));
  let diff =
    query_remote_dix_diff(target_host, current_profile, target_profile);
  spinner.finish_and_clear();

  diff?.write()
}

fn query_remote_dix_diff(
  host: &RemoteHost,
  old_generation: &Path,
  new_generation: &Path,
) -> Result<QueriedDiff> {
  let old = ResolvedRemoteStorePath::resolve(host, old_generation)?;
  let new = ResolvedRemoteStorePath::resolve(host, new_generation)?;

  thread::scope(|scope| -> Result<_> {
    let old_snapshot = scope.spawn(|| old.query_snapshot());
    let new_snapshot = scope.spawn(|| new.query_snapshot());

    let old_snapshot = old_snapshot
      .join()
      .map_err(|_| report!("old diff endpoint snapshot thread panicked"))??;
    let new_snapshot = new_snapshot
      .join()
      .map_err(|_| report!("new diff endpoint snapshot thread panicked"))??;

    Ok(QueriedDiff {
      old_label: old.path().to_path_buf(),
      new_label: new.path().to_path_buf(),
      report:    dix::diff_store_snapshots(&old_snapshot, &new_snapshot),
    })
  })
}

fn display_path(path: &Path) -> PathBuf {
  std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn print_dix_header_raw(old_label: &Path, new_label: &Path) {
  println!(
    "{arrows} {old}",
    arrows = Paint::new("<<<").bold(),
    old = old_label.display(),
  );
  println!(
    "{arrows} {new}",
    arrows = Paint::new(">>>").bold(),
    new = new_label.display(),
  );
}

fn write_dix_report(report: &dix::DiffReport) -> Result<()> {
  let stdout = io::stdout();
  let mut out = WriteFmt(stdout.lock());
  let wrote = dix::write_diff_report(&mut out, report)?;

  if wrote == 0 && report.size_old() == report.size_new() {
    info!("No version or size changes.");
  }

  Ok(())
}

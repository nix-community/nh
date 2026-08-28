use std::{
  fmt,
  io,
  path::{Path, PathBuf},
  thread,
};

use color_eyre::eyre::{Result, eyre};
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

enum DiffEndpoint {
  Local(PathBuf),
  Remote(ResolvedRemoteStorePath),
}

impl DiffEndpoint {
  fn label(&self) -> PathBuf {
    match self {
      Self::Local(path) => display_path(path),
      Self::Remote(root) => root.path().to_path_buf(),
    }
  }

  fn query_snapshot(&self) -> Result<dix::StoreSnapshot> {
    match self {
      Self::Local(path) => dix::query_store_snapshot(path, true),
      Self::Remote(root) => root.query_snapshot(),
    }
  }
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
  let report = dix::query_diff_report(old_generation, new_generation, true)?;

  Ok(QueriedDiff {
    old_label: display_path(old_generation),
    new_label: display_path(new_generation),
    report,
  })
}

/// Handles NixOS system diffing for local and remote rebuilds.
///
/// # Errors
///
/// Returns an error if local or remote store snapshot queries fail, or if the
/// diff report cannot be written.
pub fn handle_nixos_diff(
  diff: &DiffType,
  target_host: Option<&RemoteHost>,
  target_profile: &Path,
  actual_store_path: Option<&Path>,
  out_path: &Path,
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
    DiffType::Auto if target_host.is_none() && !target_profile.exists() => {
      warn!(
        "target profile {} does not exist, skipping dix diffing",
        target_profile.display()
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

  print_nixos_generation_diff(
    target_host,
    current_profile,
    target_profile,
    actual_store_path,
    out_path,
  )
}

fn print_nixos_generation_diff(
  target_host: Option<&RemoteHost>,
  current_profile: &Path,
  target_profile: &Path,
  actual_store_path: Option<&Path>,
  out_path: &Path,
) -> Result<()> {
  let Some(target_host) = target_host else {
    return print_dix_diff(current_profile, target_profile);
  };

  let remote_profile =
    remote_profile_path(out_path, target_profile, actual_store_path);
  let message = if remote_profile.is_some() {
    format!("Gathering system diff data from remote host '{target_host}'...")
  } else {
    format!(
      "Gathering system diff data from remote host '{target_host}' and local \
       store..."
    )
  };

  let spinner = progress::spinner(message);
  let diff = query_remote_nixos_diff(
    target_host,
    current_profile,
    target_profile,
    remote_profile,
  );
  spinner.finish_and_clear();

  diff?.write()
}

fn query_remote_nixos_diff(
  target_host: &RemoteHost,
  current_profile: &Path,
  target_profile: &Path,
  remote_profile: Option<PathBuf>,
) -> Result<QueriedDiff> {
  let old_root =
    ResolvedRemoteStorePath::resolve(target_host, current_profile)?;

  let new = remote_profile
    .map(|path| {
      ResolvedRemoteStorePath::resolve(target_host, &path)
        .map(DiffEndpoint::Remote)
    })
    .transpose()?
    .unwrap_or_else(|| DiffEndpoint::Local(target_profile.to_path_buf()));

  query_endpoint_diff(&DiffEndpoint::Remote(old_root), &new)
}

fn query_endpoint_diff(
  old: &DiffEndpoint,
  new: &DiffEndpoint,
) -> Result<QueriedDiff> {
  thread::scope(|scope| -> Result<_> {
    let old_snapshot = scope.spawn(|| old.query_snapshot());
    let new_snapshot = scope.spawn(|| new.query_snapshot());

    let old_snapshot = old_snapshot
      .join()
      .map_err(|_| eyre!("old diff endpoint snapshot thread panicked"))??;
    let new_snapshot = new_snapshot
      .join()
      .map_err(|_| eyre!("new diff endpoint snapshot thread panicked"))??;

    Ok(QueriedDiff {
      old_label: old.label(),
      new_label: new.label(),
      report:    dix::diff_store_snapshots(&old_snapshot, &new_snapshot),
    })
  })
}

fn remote_profile_path(
  out_path: &Path,
  target_profile: &Path,
  actual_store_path: Option<&Path>,
) -> Option<PathBuf> {
  let actual_store_path = actual_store_path?;
  let suffix = target_profile.strip_prefix(out_path).ok()?;

  if suffix.as_os_str().is_empty() {
    Some(actual_store_path.to_path_buf())
  } else {
    Some(actual_store_path.join(suffix))
  }
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn remote_profile_path_uses_store_path_for_base_profile() {
    let out_path = Path::new("result");
    let actual = Path::new("/nix/store/abc-system");

    assert_eq!(
      remote_profile_path(out_path, out_path, Some(actual)).as_deref(),
      Some(actual)
    );
  }

  #[test]
  fn remote_profile_path_preserves_specialisation_suffix() {
    let out_path = Path::new("result");
    let target_profile = Path::new("result/specialisation/foo");
    let actual = Path::new("/nix/store/abc-system");

    assert_eq!(
      remote_profile_path(out_path, target_profile, Some(actual)).as_deref(),
      Some(Path::new("/nix/store/abc-system/specialisation/foo"))
    );
  }
}

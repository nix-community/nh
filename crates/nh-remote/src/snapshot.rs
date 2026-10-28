use std::path::Path;

use misstep::{OptionExt, Result, ResultExt};

use super::{RemoteHost, run_remote_command};

/// Resolves a remote path such as `/run/current-system` to its store path.
///
/// Store paths are returned unchanged. Other paths are resolved on the remote
/// host with `readlink -f`.
///
/// # Returns
///
/// The store path that `path` resolves to on `host`.
///
/// # Errors
///
/// Returns an error if the remote path cannot be resolved or does not resolve
/// to a store path.
pub fn resolve_remote_store_path(
  host: &RemoteHost,
  path: &Path,
) -> Result<dix::StorePath> {
  let path = path
    .to_str()
    .context("remote path contains invalid UTF-8")?;
  if let Ok(store_path) = dix::StorePath::try_from(path.to_owned()) {
    return Ok(store_path);
  }

  let resolved =
    run_remote_command(host, &["readlink", "-f", "--", path], true)?
      .unwrap_or_default();
  dix::StorePath::try_from(resolved).with_context(|| {
    format!("remote path '{path}' does not resolve to a store path on '{host}'")
  })
}

/// Snapshots the closure of a remote store path with `dix snapshot`.
///
/// # Returns
///
/// The closure of `path` on `host` with NAR sizes, and the packages it
/// selects.
///
/// # Errors
///
/// Returns an error if dix is missing on the host, too old to provide
/// `dix snapshot`, or prints an incompatible snapshot.
pub fn query_remote_snapshot(
  host: &RemoteHost,
  path: &dix::StorePath,
) -> Result<dix::SnapshotDocument> {
  let path = path.as_str();
  let output = run_remote_command(host, &["dix", "snapshot", path], true)
    .with_context(|| {
      format!(
        "Could not snapshot '{path}' with dix on '{host}'. Diffing against a \
         remote host requires dix >= 3.0.0 on it, e.g. in \
         `environment.systemPackages`"
      )
    })?
    .unwrap_or_default();

  dix::SnapshotDocument::from_json(&output).with_context(|| {
    format!("dix on '{host}' returned an incompatible snapshot of '{path}'")
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  const BASH: &str = "/nix/store/00000000000000000000000000000001-bash-5.3";

  #[test]
  fn resolve_remote_store_path_preserves_store_path() -> Result<()> {
    let host = RemoteHost::parse("target.example")?;

    assert_eq!(
      resolve_remote_store_path(&host, Path::new(BASH))?.as_str(),
      BASH
    );

    Ok(())
  }
}

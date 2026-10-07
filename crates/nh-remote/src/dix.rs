use std::path::{Path, PathBuf};

use misstep::{OptionExt, Result, ResultExt, bail};

use super::{RemoteHost, run_remote_command};

/// Resolves a remote path such as `/run/current-system` to its store path.
///
/// Direct store entries are returned unchanged. Other paths are resolved on
/// the remote host with `readlink -f`.
///
/// # Returns
///
/// The store path that `path` resolves to on `host`.
///
/// # Errors
///
/// Returns an error if the remote path cannot be resolved or resolves outside
/// `/nix/store`.
pub fn resolve_remote_store_path(
  host: &RemoteHost,
  path: &Path,
) -> Result<PathBuf> {
  if path.parent() == Some(Path::new("/nix/store")) {
    return Ok(path.to_path_buf());
  }

  let path_str = path
    .to_str()
    .context("remote path contains invalid UTF-8")?;
  let output =
    run_remote_command(host, &["readlink", "-f", "--", path_str], true)?
      .unwrap_or_default();
  let resolved = PathBuf::from(output);

  if resolved
    .parent()
    .is_none_or(|parent| !parent.starts_with("/nix/store"))
  {
    bail!(
      "resolved remote path '{}' for '{}' is not in /nix/store",
      resolved.display(),
      path.display()
    );
  }

  Ok(resolved)
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
  path: &Path,
) -> Result<dix::StoreSnapshot> {
  let path = path
    .to_str()
    .context("remote path contains invalid UTF-8")?;
  let output = run_remote_command(host, &["dix", "snapshot", path], true)
    .with_context(|| {
      format!(
        "Could not snapshot '{path}' with dix on '{host}'. Diffing against a \
         remote host requires dix >= 3.0.0 on it, e.g. in \
         `environment.systemPackages`"
      )
    })?
    .unwrap_or_default();

  let document =
    dix::SnapshotDocument::from_json(&output).with_context(|| {
      format!("dix on '{host}' returned an incompatible snapshot of '{path}'")
    })?;
  Ok(document.snapshot)
}

#[cfg(test)]
mod tests {
  use super::*;

  const BASH: &str = "/nix/store/00000000000000000000000000000001-bash-5.3";

  #[test]
  fn resolve_remote_store_path_preserves_direct_store_entry() -> Result<()> {
    let host = RemoteHost::parse("target.example")?;

    assert_eq!(
      resolve_remote_store_path(&host, Path::new(BASH))?,
      Path::new(BASH)
    );

    Ok(())
  }
}

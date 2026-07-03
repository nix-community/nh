use std::path::{Path, PathBuf};

use color_eyre::{
  Result,
  eyre::{bail, eyre},
};

use super::{RemoteHost, get_nix_sshopts_env, run_remote_command};

/// A remote store path after resolving symlinks such as
/// `/run/current-system`.
#[derive(Debug, Clone)]
pub struct ResolvedRemoteStorePath {
  host: RemoteHost,
  path: PathBuf,
}

impl ResolvedRemoteStorePath {
  /// Resolve a remote path to the store path that should be queried.
  ///
  /// Direct store entries are returned unchanged. Other paths are resolved on
  /// the remote host with `readlink -f` and validated as Nix store paths.
  ///
  /// # Errors
  ///
  /// Returns an error if the remote path cannot be resolved or resolves outside
  /// `/nix/store`.
  pub fn resolve(host: &RemoteHost, path: &Path) -> Result<Self> {
    if path.parent() == Some(Path::new("/nix/store")) {
      return Ok(Self {
        host: host.clone(),
        path: path.to_path_buf(),
      });
    }

    let path = path
      .to_str()
      .ok_or_else(|| eyre!("remote path contains invalid UTF-8"))?;
    let output =
      run_remote_command(host, &["readlink", "-f", "--", path], true)?
        .ok_or_else(|| eyre!("readlink did not return a resolved path"))?;
    let mut paths = output.lines();
    let resolved_path = paths
      .next()
      .ok_or_else(|| eyre!("readlink did not return a resolved path"))?;

    if paths.next().is_some() {
      bail!("readlink returned multiple paths for one requested path");
    }

    Self::new(host, PathBuf::from(resolved_path), path)
  }

  #[must_use]
  pub fn path(&self) -> &Path {
    &self.path
  }

  /// Query this resolved remote Nix store path and convert it to dix's snapshot
  /// model.
  ///
  /// # Errors
  ///
  /// Returns an error if Nix cannot query the remote store or returns invalid
  /// JSON/path data.
  pub fn query_snapshot(&self) -> Result<dix::StoreSnapshot> {
    let backend = dix::CommandBackend::default()
      .store_url(self.host.nix_store_uri())
      .env("NIX_SSHOPTS", get_nix_sshopts_env());
    dix::query_store_snapshot_with_backend(&backend, self.path())
  }

  fn new(host: &RemoteHost, path: PathBuf, original: &str) -> Result<Self> {
    if !path.starts_with("/nix/store") {
      bail!(
        "resolved remote path '{}' for '{}' is not in /nix/store",
        path.display(),
        original
      );
    }

    Ok(Self {
      host: host.clone(),
      path,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const BASH: &str = "/nix/store/2123456789abcdefghijklmnopqrstuv-bash-5.3";

  #[test]
  fn resolved_remote_store_path_preserves_direct_store_entry() -> Result<()> {
    let host = RemoteHost::parse("target.example")?;

    let root = ResolvedRemoteStorePath::resolve(&host, Path::new(BASH))?;

    assert_eq!(root.path(), Path::new(BASH));

    Ok(())
  }
}

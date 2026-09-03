use std::{
  env,
  fs,
  io::ErrorKind,
  path::{Path, PathBuf},
};

use color_eyre::eyre::{Context, Result, bail, eyre};
use nix_command::NixCommand;
use tracing::{debug, warn};

use crate::{CommandContext, Installable};

const HELP_HINT: &str =
  "See 'man nh' or https://github.com/nix-community/nh for more details.";

pub fn installable_for(context: CommandContext) -> Result<Installable> {
  match context {
    CommandContext::Os => {
      os_installable_at(Path::new("/etc/nixos"), find_nixos_system)
    },
    CommandContext::Home => home_installable(),
    CommandContext::Darwin => {
      flake_installable(
        Path::new("/etc/nix-darwin"),
        "darwin",
        "NH_DARWIN_FLAKE",
      )
    },
  }
}

pub fn resolve_file(context: CommandContext, path: PathBuf) -> PathBuf {
  if !matches!(context, CommandContext::Os) || !path.is_dir() {
    return path;
  }

  ["system.nix", "default.nix"]
    .into_iter()
    .map(|name| path.join(name))
    .find(|candidate| candidate.is_file())
    .unwrap_or(path)
}

fn home_installable() -> Result<Installable> {
  let home =
    env::var("HOME").map_err(|_| eyre!("HOME environment variable not set"))?;
  flake_installable(
    &PathBuf::from(home).join(".config/home-manager"),
    "home",
    "NH_HOME_FLAKE",
  )
}

fn flake_installable(
  directory: &Path,
  command: &str,
  environment: &str,
) -> Result<Installable> {
  find_flake(directory, command, environment)?.ok_or_else(|| {
    eyre!(
      "No installable specified and no flake found at {}/flake.nix.\nPlease \
       either:\n- Pass a flake path as an argument (e.g., 'nh {command} \
       switch .')\n- Set the NH_FLAKE environment variable\n- Set the \
       {environment} environment variable\n\n{HELP_HINT}",
      directory.display()
    )
  })
}

fn find_flake(
  directory: &Path,
  command: &str,
  environment: &str,
) -> Result<Option<Installable>> {
  match resolve_flake_directory(directory) {
    Ok(resolved) => {
      warn!(
        "No installable was specified, falling back to {}",
        resolved.display()
      );
      Ok(Some(Installable::Flake {
        reference: resolved
          .to_str()
          .ok_or_else(|| {
            eyre!(
              "Resolved path {} contains invalid UTF-8",
              resolved.display()
            )
          })?
          .to_owned(),
        attribute: vec![],
      }))
    },
    Err(FallbackError::PermissionDenied(path)) => {
      bail!(
        "Permission denied accessing {}.\nPlease either:\n- Pass a flake path \
         as an argument (e.g., 'nh {command} switch .')\n- Set the NH_FLAKE \
         environment variable\n- Set the {environment} environment \
         variable\n\n{HELP_HINT}",
        path.display()
      )
    },
    Err(FallbackError::Io(error)) => {
      bail!(
        "I/O error accessing {}: {}\n\n{HELP_HINT}",
        directory.display(),
        error
      )
    },
    Err(FallbackError::NotFound) => {
      debug!(
        path = %directory.join("flake.nix").display(),
        "Flake entrypoint is unavailable"
      );
      Ok(None)
    },
  }
}

fn os_installable_at(
  system_directory: &Path,
  find_system: impl FnOnce() -> Result<Option<PathBuf>>,
) -> Result<Installable> {
  if let Some(installable) = find_flake(system_directory, "os", "NH_OS_FLAKE")?
  {
    return Ok(installable);
  }

  if let Some(path) = find_system()? {
    debug!(path = %path.display(), "Using <nixos-system>");
    return Ok(file(path));
  }

  let system_nix = system_directory.join("system.nix");
  if system_nix.is_file() {
    debug!(path = %system_nix.display(), "Using system.nix");
    return Ok(file(system_nix));
  }

  Ok(file(PathBuf::from("<nixpkgs/nixos>")))
}

const fn file(path: PathBuf) -> Installable {
  Installable::File {
    path,
    attribute: vec![],
  }
}

/// Ask Nix to resolve its `nixos-system` lookup-path entry.
///
/// A non-zero status means the optional entry is unavailable, matching
/// nixos-rebuild. Failure to launch Nix or malformed successful output is an
/// error rather than an absent entry.
fn find_nixos_system() -> Result<Option<PathBuf>> {
  let output = NixCommand::nix_instantiate()
    .args(["--find-file", "nixos-system"])
    .output()
    .wrap_err("failed to query the <nixos-system> Nix path entry")?;

  if !output.status.success() {
    debug!(stderr = %String::from_utf8_lossy(&output.stderr), "<nixos-system> is unavailable");
    return Ok(None);
  }

  let path = String::from_utf8(output.stdout)
    .wrap_err("nix-instantiate returned a non-UTF-8 <nixos-system> path")?;
  let path = path.trim();
  if path.is_empty() {
    bail!("nix-instantiate returned an empty <nixos-system> path");
  }
  Ok(Some(PathBuf::from(path)))
}

enum FallbackError {
  NotFound,
  PermissionDenied(PathBuf),
  Io(std::io::Error),
}

fn resolve_flake_directory(
  dir: &Path,
) -> std::result::Result<PathBuf, FallbackError> {
  let dir_is_symlink = dir.is_symlink();
  let resolved_dir = fs::canonicalize(dir).map_err(|error| {
    match error.kind() {
      ErrorKind::NotFound => FallbackError::NotFound,
      ErrorKind::PermissionDenied => {
        FallbackError::PermissionDenied(dir.to_path_buf())
      },
      _ => FallbackError::Io(error),
    }
  })?;

  let flake = resolved_dir.join("flake.nix");
  if dir_is_symlink {
    return require_file(&flake).map(|()| resolved_dir);
  }

  if flake.is_symlink() {
    return fs::canonicalize(&flake)
      .map_err(|error| io_error(error, &flake))?
      .parent()
      .map(Path::to_path_buf)
      .ok_or(FallbackError::NotFound);
  }

  require_file(&flake).map(|()| resolved_dir)
}

pub fn validate_flake_reference(
  reference: &str,
  path: &Path,
  environment: &str,
) -> Result<()> {
  match resolve_flake_directory(path) {
    Ok(_) => Ok(()),
    Err(FallbackError::NotFound) => {
      bail!(
        "Flake reference `{reference}` points to local path `{}`, but that \
         path does not exist or does not contain a flake.nix file.\nPass an \
         existing flake path or update NH_FLAKE/{environment} if this value \
         came from the environment.",
        path.display()
      )
    },
    Err(FallbackError::PermissionDenied(path)) => {
      bail!(
        "Permission denied accessing {} while checking flake reference \
         `{reference}`.",
        path.display()
      )
    },
    Err(FallbackError::Io(source)) => {
      bail!(
        "I/O error checking flake reference `{reference}` at {}: {source}",
        path.display()
      )
    },
  }
}

fn require_file(path: &Path) -> std::result::Result<(), FallbackError> {
  match fs::metadata(path) {
    Ok(metadata) if metadata.is_file() => Ok(()),
    Ok(_) => Err(FallbackError::NotFound),
    Err(error) => Err(io_error(error, path)),
  }
}

fn io_error(error: std::io::Error, path: &Path) -> FallbackError {
  match error.kind() {
    ErrorKind::NotFound => FallbackError::NotFound,
    ErrorKind::PermissionDenied => {
      FallbackError::PermissionDenied(path.to_path_buf())
    },
    _ => FallbackError::Io(error),
  }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "Fine in tests")]
mod tests {
  use super::*;
  use crate::InstallableArgs;

  #[test]
  fn system_nix_directory_resolution_is_os_only() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("default.nix"), "{}").unwrap();
    fs::write(dir.path().join("system.nix"), "{}").unwrap();

    for (context, expected) in [
      (CommandContext::Os, dir.path().join("system.nix")),
      (CommandContext::Home, dir.path().to_path_buf()),
      (CommandContext::Darwin, dir.path().to_path_buf()),
    ] {
      let args = InstallableArgs::Specified(file(dir.path().to_path_buf()));
      let installable = args.resolve_or_default(context).unwrap();
      assert!(matches!(
        installable,
        Installable::File { path, attribute }
          if path == expected && attribute.is_empty()
      ));
    }
  }

  #[test]
  fn nixos_system_precedes_etc_system_nix() {
    let etc = tempfile::tempdir().unwrap();
    fs::write(etc.path().join("system.nix"), "{}").unwrap();
    let selected = PathBuf::from("/resolved/system.nix");

    let installable =
      os_installable_at(etc.path(), || Ok(Some(selected.clone()))).unwrap();
    assert!(matches!(
      installable,
      Installable::File { path, attribute }
        if path == selected && attribute.is_empty()
    ));
  }

  #[test]
  fn system_directory_flake_precedes_non_flake_defaults() {
    let etc = tempfile::tempdir().unwrap();
    fs::write(etc.path().join("flake.nix"), "{}").unwrap();
    fs::write(etc.path().join("system.nix"), "{}").unwrap();

    let installable = os_installable_at(etc.path(), || {
      Err(eyre!(
        "non-flake lookup must not run when /etc/nixos is a flake"
      ))
    })
    .unwrap();

    assert!(matches!(
      installable,
      Installable::Flake { reference, attribute }
        if reference == etc.path().to_string_lossy() && attribute.is_empty()
    ));
  }

  #[test]
  fn etc_system_nix_precedes_nixpkgs_fallback() {
    let etc = tempfile::tempdir().unwrap();
    let system_nix = etc.path().join("system.nix");
    fs::write(&system_nix, "{}").unwrap();

    let installable = os_installable_at(etc.path(), || Ok(None)).unwrap();
    assert!(matches!(
      installable,
      Installable::File { path, attribute }
        if path == system_nix && attribute.is_empty()
    ));
  }

  #[test]
  fn nixpkgs_is_final_nixos_default() {
    let etc = tempfile::tempdir().unwrap();
    let installable = os_installable_at(etc.path(), || Ok(None)).unwrap();
    assert!(matches!(
      installable,
      Installable::File { path, attribute }
        if path == Path::new("<nixpkgs/nixos>") && attribute.is_empty()
    ));
  }
}

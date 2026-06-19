use std::{
  env,
  fs,
  io::{self, IsTerminal, Write},
  os::unix::fs::{DirBuilderExt, PermissionsExt},
  path::{Path, PathBuf},
};

use color_eyre::{
  Result,
  eyre::{Context, bail},
};
use secrecy::{ExposeSecret, SecretString};
use yansi::Paint;

const TOKEN_CREATION_URL: &str =
  "https://github.com/settings/personal-access-tokens/new";
const TOKEN_ENV: &str = "GH_TOKEN";
const TOKEN_FILE_ENV: &str = "NH_GITHUB_TOKEN_FILE";
const TOKEN_FILE: &str = "github-token";

pub fn token() -> Result<SecretString> {
  // If GH_TOKEN is set, use that.
  if let Some(token) = env::var(TOKEN_ENV)
    .ok()
    .map(|t| t.trim().to_owned())
    .filter(|t| !t.is_empty())
    .map(SecretString::from)
  {
    return Ok(token);
  }

  let token_path = token_store_path()?;
  if let Some(token) = read_token_file(&token_path)? {
    return Ok(token);
  }

  if !io::stdin().is_terminal() {
    bail!(
      "GitHub token not found; set {TOKEN_ENV} or write a token to {}",
      token_path.display()
    );
  }

  eprintln!(
    r"NH needs a GitHub token to access the GitHub API for searching pull requests and issues.
Please create a GitHub token at {}
if you do not already have one, or paste an existing token down below.
You do not need to set any scopes for your token since NH only uses it to make authenticated requests to GitHub.
The token will be saved to {} with user-only permissions.
     ",
    TOKEN_CREATION_URL.underline().blue(),
    token_path.display().green()
  );

  let token = inquire::Password::new("GitHub token:")
    .without_confirmation()
    .prompt()
    .context("failed to read GitHub token")?;
  let token = token.trim();
  if token.is_empty() {
    bail!("empty GitHub token");
  }

  let token = SecretString::new(token.to_string().into());
  write_token_file(&token_path, &token)?;
  eprintln!("Saved GitHub token to {}.", token_path.display());
  Ok(token)
}

fn token_store_path() -> Result<PathBuf> {
  let get_env = |var| -> Result<Option<PathBuf>> {
    if let Some(val) = env::var_os(var) {
      if val.is_empty() {
        bail!("{var} is set but empty");
      }
      return Ok(Some(PathBuf::from(val)));
    }
    Ok(None)
  };

  if let Some(path) = get_env(TOKEN_FILE_ENV)? {
    return Ok(path);
  }

  if let Some(state_home) = get_env("XDG_STATE_HOME")? {
    return Ok(state_home.join("nh").join(TOKEN_FILE));
  }

  if let Some(home) = get_env("HOME")? {
    return Ok(
      home
        .join(".local")
        .join("state")
        .join("nh")
        .join(TOKEN_FILE),
    );
  }

  bail!(
    "could not determine GitHub token store path; set {TOKEN_ENV} or \
     {TOKEN_FILE_ENV}"
  )
}

fn read_token_file(path: &Path) -> Result<Option<SecretString>> {
  let raw = match fs::read_to_string(path) {
    Ok(raw) => raw,
    Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
    Err(err) => {
      return Err(err)
        .with_context(|| format!("failed to read {}", path.display()));
    },
  };
  let token = raw.trim();
  Ok((!token.is_empty()).then(|| SecretString::new(token.to_string().into())))
}

/// Writes a token atomically to a specified path.
///
/// # Errors
///
/// - Returns an error if the parent directory cannot be created.
/// - Returns an error if the token cannot be written to a temporary file.
/// - Returns an error if the temporary file cannot persisted to the target path
fn write_token_file(path: &Path, token: &SecretString) -> Result<()> {
  let parent_dir = path.parent().ok_or_else(|| {
    color_eyre::eyre::eyre!(
      "Invalid token path {}: no parent directory found",
      path.display()
    )
  })?;

  // Ensure the parent directory exists with restrictive permissions
  fs::DirBuilder::new()
    .recursive(true)
    .mode(0o700)
    .create(parent_dir)
    .with_context(|| {
      format!("failed to create directory {}", parent_dir.display())
    })?;

  // Write to a named temporary file in the same directory to ensure atomic
  // swap.
  let mut temp_file = tempfile::NamedTempFile::new_in(parent_dir)
    .with_context(|| "failed to create temporary token file")?;

  // Set permissions on the temporary file before writing any data.
  let perms = fs::Permissions::from_mode(0o600);
  temp_file
    .as_file()
    .set_permissions(perms)
    .with_context(|| "failed to set secure permissions on temporary file")?;

  // Write the secret.
  temp_file
    .write_all(token.expose_secret().as_bytes())
    .with_context(|| "failed to write secret to temporary file")?;

  // Sync to disk, just to make sure.
  temp_file
    .as_file()
    .sync_all()
    .with_context(|| "failed to sync token file to disk")?;

  // Atomically replace the old file with the new one.
  temp_file.persist(path).with_context(|| {
    format!("failed to atomically save token to {}", path.display())
  })?;

  Ok(())
}

#[cfg(test)]
mod tests {
  use std::{env, fs, os::unix::fs::PermissionsExt};

  use color_eyre::{Result, eyre::ContextCompat};
  use secrecy::ExposeSecret;
  use serial_test::serial;
  use tempfile::tempdir;

  use super::*;

  struct EnvGuard {
    key:   &'static str,
    value: Option<std::ffi::OsString>,
  }

  impl EnvGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
      let guard = Self {
        key,
        value: env::var_os(key),
      };
      unsafe {
        env::set_var(key, value);
      }
      guard
    }

    fn remove(key: &'static str) -> Self {
      let guard = Self {
        key,
        value: env::var_os(key),
      };
      unsafe {
        env::remove_var(key);
      }
      guard
    }
  }

  impl Drop for EnvGuard {
    fn drop(&mut self) {
      unsafe {
        if let Some(value) = &self.value {
          env::set_var(self.key, value);
        } else {
          env::remove_var(self.key);
        }
      }
    }
  }

  #[test]
  #[serial]
  fn gh_token_wins_over_token_file() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join(TOKEN_FILE);
    fs::write(&path, "from-file")?;
    let _token = EnvGuard::set(TOKEN_ENV, "  from-env  ");
    let _token_file = EnvGuard::set(TOKEN_FILE_ENV, &path);

    let token = token()?;

    assert_eq!("from-env", token.expose_secret());
    Ok(())
  }

  #[test]
  #[serial]
  fn token_file_env_overrides_default_path() -> Result<()> {
    let dir = tempdir()?;
    let state = tempdir()?;
    let path = dir.path().join(TOKEN_FILE);
    fs::write(&path, "  from-file  ")?;
    let _token = EnvGuard::remove(TOKEN_ENV);
    let _token_file = EnvGuard::set(TOKEN_FILE_ENV, &path);
    let _state = EnvGuard::set("XDG_STATE_HOME", state.path());

    let token = token()?;

    assert_eq!("from-file", token.expose_secret());
    Ok(())
  }

  #[test]
  #[serial]
  fn empty_token_file_env_errors() {
    let _token = EnvGuard::remove(TOKEN_ENV);
    let _token_file = EnvGuard::set(TOKEN_FILE_ENV, "");

    #[expect(clippy::expect_used)]
    let err = token_store_path().expect_err("empty override should fail");

    assert!(err.to_string().contains(TOKEN_FILE_ENV));
  }

  #[test]
  #[serial]
  fn default_token_path_uses_xdg_state_home() -> Result<()> {
    let dir = tempdir()?;
    let _token_file = EnvGuard::remove(TOKEN_FILE_ENV);
    let _state = EnvGuard::set("XDG_STATE_HOME", dir.path());

    assert_eq!(dir.path().join("nh").join(TOKEN_FILE), token_store_path()?);
    Ok(())
  }

  #[test]
  #[serial]
  fn default_token_path_falls_back_to_local_state() -> Result<()> {
    let dir = tempdir()?;
    let _token_file = EnvGuard::remove(TOKEN_FILE_ENV);
    let _state = EnvGuard::remove("XDG_STATE_HOME");
    let _home = EnvGuard::set("HOME", dir.path());

    assert_eq!(
      dir
        .path()
        .join(".local")
        .join("state")
        .join("nh")
        .join(TOKEN_FILE),
      token_store_path()?
    );
    Ok(())
  }

  #[test]
  fn write_token_file_creates_private_file() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("state").join(TOKEN_FILE);
    let token = SecretString::new("ghp_test".into());

    write_token_file(&path, &token)?;

    assert_eq!("ghp_test", fs::read_to_string(&path)?);
    let file_mode = fs::metadata(&path)?.permissions().mode();
    assert_eq!(0, file_mode & 0o077);
    let dir_mode = fs::metadata(
      path
        .parent()
        .context("token test path should have a parent")?,
    )?
    .permissions()
    .mode();
    assert_eq!(0, dir_mode & 0o077);
    Ok(())
  }
}

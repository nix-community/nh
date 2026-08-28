use std::{
  env,
  fs,
  io::{self, Write},
  os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
  path::{Path, PathBuf},
};

use color_eyre::{
  Result,
  eyre::{Context, bail},
};
use toml_edit::DocumentMut;

const CONFIG_ENV: &str = "NH_CONFIG";
const CONFIG_FILE: &str = "config.toml";

#[derive(Debug)]
pub struct ConfigStore {
  path:     PathBuf,
  document: DocumentMut,
}

#[derive(Debug, Clone, Default)]
pub struct Config {}

impl ConfigStore {
  /// Load NH configuration from the default path.
  ///
  /// # Errors
  ///
  /// Returns an error when the default path cannot be determined, the file
  /// cannot be read, or the TOML document is malformed.
  pub fn load_default() -> Result<Self> {
    Self::load_from(default_config_path()?)
  }

  /// Load NH configuration from a specific path.
  ///
  /// Missing files are treated as an empty configuration and are only created
  /// when [`Self::save`] is called.
  ///
  /// # Errors
  ///
  /// Returns an error when the file cannot be read or parsed.
  pub fn load_from(path: impl Into<PathBuf>) -> Result<Self> {
    let path = path.into();
    let document = match fs::read_to_string(&path) {
      Ok(raw) => parse_document(&path, &raw)?,
      Err(err) if err.kind() == io::ErrorKind::NotFound => DocumentMut::new(),
      Err(err) => {
        return Err(err)
          .with_context(|| format!("failed to read {}", path.display()));
      },
    };

    Ok(Self { path, document })
  }

  #[must_use]
  pub fn path(&self) -> &Path {
    &self.path
  }

  /// Return the typed view of the known NH configuration fields.
  ///
  /// # Errors
  ///
  /// Returns an error when a known field is present with the wrong type.
  pub const fn config(&self) -> Result<Config> {
    Ok(Config {})
  }

  /// Save the document, creating parent directories as needed.
  ///
  /// # Errors
  ///
  /// Returns an error when the parent directory cannot be created or the file
  /// cannot be written.
  pub fn save(&self) -> Result<()> {
    write_private(&self.path, self.document.to_string().as_bytes())
  }
}

/// Resolve the path to NH configuration.
///
/// # Errors
///
/// Returns an error when `NH_CONFIG` is empty or no home directory can be
/// determined for the fallback path.
pub fn default_config_path() -> Result<PathBuf> {
  if let Some(path) = env::var_os(CONFIG_ENV) {
    if path.is_empty() {
      bail!("{CONFIG_ENV} is set but empty");
    }

    return Ok(PathBuf::from(path));
  }

  if let Some(config_home) = non_empty_var("XDG_CONFIG_HOME") {
    return Ok(PathBuf::from(config_home).join("nh").join(CONFIG_FILE));
  }

  if let Some(home) = non_empty_var("HOME") {
    return Ok(
      PathBuf::from(home)
        .join(".config")
        .join("nh")
        .join(CONFIG_FILE),
    );
  }

  bail!("could not determine NH configuration path; set {CONFIG_ENV}")
}

fn parse_document(path: &Path, raw: &str) -> Result<DocumentMut> {
  raw.parse::<DocumentMut>().with_context(|| {
    format!("failed to parse NH configuration at {}", path.display())
  })
}

fn non_empty_var(name: &str) -> Option<std::ffi::OsString> {
  env::var_os(name).filter(|value| !value.is_empty())
}

fn write_private(path: &Path, contents: &[u8]) -> Result<()> {
  if let Some(parent) = path.parent() {
    create_config_dir(parent)?;
  }

  let mut options = fs::OpenOptions::new();
  options.create(true).write(true).truncate(true).mode(0o600);

  let mut file = options
    .open(path)
    .with_context(|| format!("failed to open {}", path.display()))?;
  file
    .write_all(contents)
    .with_context(|| format!("failed to write {}", path.display()))?;

  set_user_only_file(path)?;
  Ok(())
}

fn create_config_dir(path: &Path) -> Result<()> {
  let mut builder = fs::DirBuilder::new();
  builder.recursive(true).mode(0o700);
  builder
    .create(path)
    .with_context(|| format!("failed to create {}", path.display()))
}

fn set_user_only_file(path: &Path) -> Result<()> {
  fs::set_permissions(path, fs::Permissions::from_mode(0o600)).with_context(
    || format!("failed to set private permissions on {}", path.display()),
  )
}

#[cfg(test)]
mod tests {
  use std::{env, fs, os::unix::fs::PermissionsExt};

  use color_eyre::Result;
  use serial_test::serial;
  use tempfile::tempdir;

  use super::{ConfigStore, default_config_path};

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
  fn missing_file_loads_as_default_config() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("config.toml");

    let store = ConfigStore::load_from(&path)?;

    let _config = store.config()?;
    assert!(!path.exists());
    Ok(())
  }

  #[test]
  fn save_preserves_comments_and_unknown_fields() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("config.toml");
    fs::write(&path, "# keep me\n[unknown]\nvalue = 1\n")?;

    let store = ConfigStore::load_from(&path)?;
    store.save()?;

    let written = fs::read_to_string(&path)?;
    assert!(written.contains("# keep me"));
    assert!(written.contains("[unknown]"));
    assert!(written.contains("value = 1"));
    Ok(())
  }

  #[test]
  fn save_creates_private_file() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("nh").join("config.toml");
    let store = ConfigStore::load_from(&path)?;
    store.save()?;

    let mode = fs::metadata(&path)?.permissions().mode();
    assert_eq!(0, mode & 0o077);
    Ok(())
  }

  #[test]
  #[serial]
  fn nh_config_overrides_default_path() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("custom.toml");
    let _config = EnvGuard::set("NH_CONFIG", &path);

    assert_eq!(path, default_config_path()?);
    Ok(())
  }

  #[test]
  #[serial]
  fn xdg_config_home_falls_back_when_no_override_exists() -> Result<()> {
    let dir = tempdir()?;
    let _config = EnvGuard::remove("NH_CONFIG");
    let _xdg = EnvGuard::set("XDG_CONFIG_HOME", dir.path());

    assert_eq!(
      dir.path().join("nh").join("config.toml"),
      default_config_path()?
    );
    Ok(())
  }
}

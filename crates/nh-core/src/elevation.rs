use std::{
  collections::HashMap,
  convert::Infallible,
  path::{Path, PathBuf},
  str::FromStr,
  sync::{Mutex, OnceLock},
};

use color_eyre::{
  Result,
  eyre::{self, Context, bail},
};
use secrecy::{ExposeSecret, SecretString};
use tracing::{debug, warn};
use which::which;

static PASSWORD_CACHE: OnceLock<Mutex<HashMap<String, SecretString>>> =
  OnceLock::new();

/// Strategy argument for handling privilege elevation when running commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElevationStrategyArg {
  /// No elevation - commands run without privilege escalation.
  None,

  /// Automatically detect and use the first available elevation program.
  Auto,

  /// Use elevation without interactive password prompting for remote hosts
  /// with NOPASSWD configured.
  Passwordless,

  /// Use remote sudo stdin authentication with an empty password line.
  EmptyPassword,

  /// Use the specified elevation program.
  Program(PathBuf),
}

impl FromStr for ElevationStrategyArg {
  type Err = Infallible;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "none" => Ok(Self::None),
      "auto" => Ok(Self::Auto),
      "passwordless" => Ok(Self::Passwordless),
      "empty-password" => Ok(Self::EmptyPassword),
      _ => {
        s.strip_prefix("program:").map_or_else(
          || Ok(Self::Program(PathBuf::from(s))),
          |rest| Ok(Self::Program(PathBuf::from(rest))),
        )
      },
    }
  }
}

/// Complete elevation policy selected from CLI/configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elevation {
  strategy:  ElevationStrategy,
  sudo_auth: SudoAuth,
}

impl Elevation {
  #[must_use]
  pub const fn new(strategy: ElevationStrategy) -> Self {
    Self {
      strategy,
      sudo_auth: SudoAuth::Prompt,
    }
  }

  #[must_use]
  pub const fn strategy(&self) -> &ElevationStrategy {
    &self.strategy
  }

  #[must_use]
  pub const fn uses_local_sudo_askpass(&self) -> bool {
    !matches!(self.sudo_auth, SudoAuth::NoStdin)
  }

  #[must_use]
  pub const fn requires_ssh_agent_forwarding(&self) -> bool {
    matches!(self.sudo_auth, SudoAuth::EmptyPassword)
  }

  /// Resolve remote sudo authentication data, including stdin and SSH
  /// transport requirements.
  ///
  /// # Errors
  ///
  /// Returns an error if reading or caching an interactive sudo password fails.
  pub fn remote_sudo_auth(&self, host: &str) -> Result<RemoteSudoAuth> {
    if matches!(self.strategy, ElevationStrategy::None) {
      Ok(RemoteSudoAuth::none())
    } else {
      Ok(RemoteSudoAuth {
        stdin:             self.sudo_auth.stdin_for_host(host)?,
        forward_ssh_agent: self.requires_ssh_agent_forwarding(),
      })
    }
  }

  /// Build the remote elevation command prefix, if elevation is enabled.
  ///
  /// # Errors
  ///
  /// Returns an error when the selected program cannot support the requested
  /// remote authentication policy.
  pub fn remote_command(&self) -> Result<Option<RemoteElevationCommand>> {
    let Some(program) = self.strategy.remote_program_name()? else {
      return Ok(None);
    };

    let args = match (program.as_str(), self.sudo_auth) {
      ("sudo", auth) => auth.sudo_args(),
      ("doas", SudoAuth::NoStdin) => &["-n"],
      ("doas", _) => {
        bail!(
          "doas does not support stdin password input for remote deployment. \
           Use --elevation-strategy=passwordless if remote has NOPASSWD \
           configured."
        )
      },
      ("run0", SudoAuth::NoStdin) => &["--no-ask-password"],
      ("run0", _) => {
        bail!(
          "run0 does not support stdin password input for remote deployment. \
           Use --elevation-strategy=passwordless if authentication is not \
           required."
        )
      },
      ("pkexec", _) => {
        bail!(
          "pkexec does not support non-interactive password input for remote \
           deployment. pkexec requires a polkit agent which is not available \
           over SSH."
        )
      },
      (_, SudoAuth::NoStdin) => {
        bail!(
          "Unknown elevation program '{}' does not have known passwordless \
           support. Only sudo, doas, and run0 are supported with \
           --elevation-strategy=passwordless",
          program
        )
      },
      (..) => {
        bail!(
          "Unknown elevation program '{}' does not support stdin password \
           input for remote deployment. Only sudo supports password input \
           over SSH. Use --elevation-strategy=passwordless if remote has \
           passwordless elevation configured, or use a known elevation \
           program (sudo/doas/run0).",
          program
        )
      },
    };

    Ok(Some(RemoteElevationCommand { program, args }))
  }
}

impl Default for Elevation {
  fn default() -> Self {
    Self::new(ElevationStrategy::Auto)
  }
}

impl From<ElevationStrategy> for Elevation {
  fn from(strategy: ElevationStrategy) -> Self {
    Self::new(strategy)
  }
}

impl From<ElevationStrategyArg> for Elevation {
  fn from(arg: ElevationStrategyArg) -> Self {
    match arg {
      ElevationStrategyArg::None => {
        Self {
          strategy:  ElevationStrategy::None,
          sudo_auth: SudoAuth::NoStdin,
        }
      },
      ElevationStrategyArg::Auto => Self::default(),
      ElevationStrategyArg::Passwordless => {
        Self {
          strategy:  ElevationStrategy::Auto,
          sudo_auth: SudoAuth::NoStdin,
        }
      },
      ElevationStrategyArg::EmptyPassword => {
        Self {
          strategy:  ElevationStrategy::Force("sudo"),
          sudo_auth: SudoAuth::EmptyPassword,
        }
      },
      ElevationStrategyArg::Program(path) => {
        Self::new(ElevationStrategy::Prefer(path))
      },
    }
  }
}

/// Strategy for selecting a privilege elevation program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElevationStrategy {
  /// Automatically detect and use the first available elevation program.
  Auto,

  /// Try the specified elevation program first, falling back to Auto for local
  /// commands. Remote commands use the provided program name.
  Prefer(PathBuf),

  /// Use only the specified program name.
  #[allow(dead_code, reason = "In use")]
  Force(&'static str),

  /// Do not use any elevation program.
  None,
}

impl ElevationStrategy {
  /// Resolves the elevation strategy to a local program path.
  ///
  /// # Errors
  ///
  /// Returns an error when the requested local elevation program cannot be
  /// found.
  pub fn resolve(&self) -> Result<PathBuf> {
    match self {
      Self::Auto => Self::choice(),
      Self::Prefer(program) => {
        which(program).or_else(|_| {
          warn!(
            ?program,
            "Preferred elevation program not found, falling back to \
             auto-detection"
          );
          Self::choice()
        })
      },
      Self::Force(program_name) => {
        which(program_name).context(format!(
          "Forced elevation program '{program_name}' not found in PATH"
        ))
      },
      Self::None => bail!("Elevation disabled via --elevation-strategy=none"),
    }
  }

  /// Returns the program name to invoke on a remote host.
  ///
  /// Unlike local resolution, this does not require a forced or preferred
  /// program to exist on the local machine.
  ///
  /// # Errors
  ///
  /// Returns an error if no elevation program can be selected.
  pub fn remote_program_name(&self) -> Result<Option<String>> {
    match self {
      Self::None => Ok(None),
      Self::Force(program_name) => Ok(Some((*program_name).to_string())),
      Self::Prefer(program) => Ok(Some(file_name(program)?.to_string())),
      Self::Auto => {
        let program = Self::choice()?;
        Ok(Some(file_name(&program)?.to_string()))
      },
    }
  }

  /// Gets a path to a privilege elevation program based on what is available in
  /// the system.
  ///
  /// # Errors
  ///
  /// Returns an error if none of the known elevation programs can be found.
  fn choice() -> Result<PathBuf> {
    const STRATEGIES: [&str; 4] = ["doas", "sudo", "run0", "pkexec"];

    for strategy in STRATEGIES {
      if let Ok(path) = which(strategy) {
        debug!(?path, "{strategy} path found");
        return Ok(path);
      }
    }

    Err(eyre::eyre!(
      "No elevation strategy found. Checked: {}",
      STRATEGIES.join(", ")
    ))
  }
}

fn file_name(path: &Path) -> Result<&str> {
  path
    .file_name()
    .and_then(|name| name.to_str())
    .ok_or_else(|| eyre::eyre!("Failed to determine elevation program name"))
}

/// Remote sudo authentication policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SudoAuth {
  /// Prompt for a non-empty sudo password and send it over SSH stdin.
  Prompt,

  /// Do not send stdin. The remote elevation command must not require a
  /// password.
  NoStdin,

  /// Send an intentionally empty password line for remote sudo setups that
  /// authenticate out-of-band.
  EmptyPassword,
}

impl SudoAuth {
  const fn sudo_args(self) -> &'static [&'static str] {
    match self {
      Self::NoStdin => &["--non-interactive"],
      Self::Prompt | Self::EmptyPassword => &["--prompt=", "--stdin"],
    }
  }

  /// Resolve stdin for this remote sudo authentication policy.
  ///
  /// # Errors
  ///
  /// Returns an error if reading or caching an interactive sudo password fails.
  fn stdin_for_host(self, host: &str) -> Result<RemoteSudoStdin> {
    match self {
      Self::NoStdin => Ok(RemoteSudoStdin::None),
      Self::EmptyPassword => {
        Ok(RemoteSudoStdin::Line(SecretString::new(
          String::new().into(),
        )))
      },
      Self::Prompt => {
        if let Some(cached_password) = get_cached_password(host)? {
          return Ok(RemoteSudoStdin::Line(cached_password));
        }

        let password =
          inquire::Password::new(&format!("[sudo] password for {host}:"))
            .without_confirmation()
            .prompt()
            .context("Failed to read sudo password")?;
        if password.is_empty() {
          bail!(
            "Password cannot be empty. Use \
             --elevation-strategy=empty-password for remote sudo \
             authentication that accepts an empty stdin password."
          );
        }

        let secret_password = SecretString::new(password.into());
        cache_password(host, secret_password.clone())?;
        Ok(RemoteSudoStdin::Line(secret_password))
      },
    }
  }
}

/// Program and arguments used to prefix a remote command with elevation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteElevationCommand {
  program: String,
  args:    &'static [&'static str],
}

impl RemoteElevationCommand {
  #[must_use]
  pub fn program(&self) -> &str {
    &self.program
  }

  #[must_use]
  pub const fn args(&self) -> &'static [&'static str] {
    self.args
  }

  #[must_use]
  pub fn prefix(&self) -> String {
    let args = self.args.join(" ");
    if args.is_empty() {
      self.program.clone()
    } else {
      format!("{} {args}", self.program)
    }
  }
}

/// Remote sudo authentication data for one SSH command.
#[derive(Clone)]
pub struct RemoteSudoAuth {
  stdin:             RemoteSudoStdin,
  forward_ssh_agent: bool,
}

impl RemoteSudoAuth {
  #[must_use]
  pub const fn none() -> Self {
    Self {
      stdin:             RemoteSudoStdin::None,
      forward_ssh_agent: false,
    }
  }

  #[cfg(test)]
  #[must_use]
  pub(crate) const fn with_stdin(stdin: RemoteSudoStdin) -> Self {
    Self {
      stdin,
      forward_ssh_agent: false,
    }
  }

  #[must_use]
  pub fn stdin_bytes(&self) -> Option<Vec<u8>> {
    self.stdin.bytes()
  }

  #[must_use]
  pub const fn requires_ssh_agent_forwarding(&self) -> bool {
    self.forward_ssh_agent
  }

  #[cfg(test)]
  const fn stdin(&self) -> &RemoteSudoStdin {
    &self.stdin
  }
}

/// Explicit stdin payload for a remote elevated command.
#[derive(Clone)]
pub(crate) enum RemoteSudoStdin {
  None,
  Line(SecretString),
}

impl RemoteSudoStdin {
  #[must_use]
  fn bytes(&self) -> Option<Vec<u8>> {
    match self {
      Self::None => None,
      Self::Line(password) => {
        Some(format!("{}\n", password.expose_secret()).into_bytes())
      },
    }
  }
}

/// Retrieves a cached password for the specified host.
///
/// # Errors
///
/// Returns an error if the password cache lock is poisoned.
fn get_cached_password(host: &str) -> Result<Option<SecretString>> {
  let cache = PASSWORD_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
  let guard = cache
    .lock()
    .map_err(|_| eyre::eyre!("Password cache lock poisoned"))?;
  Ok(guard.get(host).cloned())
}

/// Stores a password in the cache for the specified host.
///
/// # Errors
///
/// Returns an error if the password cache lock is poisoned.
fn cache_password(host: &str, password: SecretString) -> Result<()> {
  let cache = PASSWORD_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

  cache
    .lock()
    .map_err(|_| eyre::eyre!("Password cache lock poisoned"))?
    .insert(host.to_string(), password);

  Ok(())
}

#[cfg(test)]
mod tests {
  #![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::unreachable,
    reason = "Fine in tests"
  )]

  use secrecy::ExposeSecret;

  use super::*;

  #[test]
  fn test_elevation_strategy_passwordless_resolves() {
    let elevation = Elevation::from(ElevationStrategyArg::Passwordless);
    let result = elevation.strategy().resolve();

    assert!(result.is_ok());
    let program = result.unwrap();
    assert!(!program.as_os_str().is_empty());
  }

  #[test]
  fn test_elevation_strategy_arg_empty_password_parsing() {
    let parsed = "empty-password".parse::<ElevationStrategyArg>();
    assert!(matches!(parsed, Ok(ElevationStrategyArg::EmptyPassword)));
  }

  #[test]
  fn test_empty_password_arg_forces_sudo_and_empty_remote_stdin() {
    let elevation = Elevation::from(ElevationStrategyArg::EmptyPassword);

    assert_eq!(elevation.strategy(), &ElevationStrategy::Force("sudo"));
    let remote_command = elevation
      .remote_command()
      .expect("empty-password remote command should build")
      .expect("empty-password should use remote elevation");
    assert_eq!(remote_command.prefix(), "sudo --prompt= --stdin");

    let auth = elevation
      .remote_sudo_auth("user@host")
      .expect("empty-password should not prompt");
    match auth.stdin() {
      RemoteSudoStdin::Line(password) => {
        assert_eq!(password.expose_secret(), "");
        assert_eq!(auth.stdin_bytes(), Some(b"\n".to_vec()));
        assert!(auth.requires_ssh_agent_forwarding());
      },
      RemoteSudoStdin::None => unreachable!("expected empty password stdin"),
    }
  }

  #[test]
  fn test_remote_sudo_auth_skips_stdin_for_none_and_passwordless() {
    let none_stdin = Elevation::from(ElevationStrategyArg::None)
      .remote_sudo_auth("user@host")
      .expect("none strategy should not prompt");
    assert!(matches!(none_stdin.stdin(), RemoteSudoStdin::None));

    let passwordless_stdin =
      Elevation::from(ElevationStrategyArg::Passwordless)
        .remote_sudo_auth("user@host")
        .expect("passwordless strategy should not prompt");
    assert!(matches!(passwordless_stdin.stdin(), RemoteSudoStdin::None));
  }

  #[test]
  fn test_elevation_strategy_arg_program_prefix_parsing() {
    let parsed = "program:/path/to/bin".parse::<ElevationStrategyArg>();
    assert!(parsed.is_ok());
    match parsed.unwrap() {
      ElevationStrategyArg::Program(path) => {
        assert_eq!(path, PathBuf::from("/path/to/bin"));
      },
      _ => unreachable!("Expected Program variant"),
    }
  }
}

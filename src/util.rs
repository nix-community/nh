use std::{
  collections::HashSet,
  fmt,
  io,
  path::Path,
  process::{Command as StdCommand, Stdio},
  str,
  sync::{LazyLock, OnceLock},
};

use color_eyre::{Result, eyre};
use regex::Regex;
use tracing::{debug, info};

use crate::commands::{Command, ElevationStrategy};

#[derive(Debug, Clone, PartialEq)]
pub enum NixVariant {
  Nix,
  Lix,
  Determinate,
}

static NIX_VARIANT: OnceLock<NixVariant> = OnceLock::new();

struct WriteFmt<W: io::Write>(W);

impl<W: io::Write> fmt::Write for WriteFmt<W> {
  fn write_str(&mut self, string: &str) -> fmt::Result {
    self.0.write_all(string.as_bytes()).map_err(|_| fmt::Error)
  }
}
/// Get the Nix variant (cached)
pub fn get_nix_variant() -> &'static NixVariant {
  NIX_VARIANT.get_or_init(|| {
    let output = Command::new("nix")
      .arg("--version")
      .run_capture()
      .ok()
      .flatten();

    // XXX: If running with dry=true or Nix is not installed, output might be
    // None The latter is less likely to occur, but we still want graceful
    // handling.
    let output_str = match output {
      Some(output) => output,
      None => return NixVariant::Nix, // default to standard Nix variant
    };

    let output_lower = output_str.to_lowercase();

    // FIXME: This fails to account for Nix variants we don't check for and
    // assumes the environment is mainstream Nix.
    if output_lower.contains("determinate") {
      NixVariant::Determinate
    } else if output_lower.contains("lix") {
      NixVariant::Lix
    } else {
      NixVariant::Nix
    }
  });

  NIX_VARIANT
    .get()
    .expect("NIX_VARIANT should be initialized by get_nix_variant")
}

// Matches and captures major, minor, and optional patch numbers from semantic
// version strings, optionally followed by a "pre" pre-release suffix.
static VERSION_REGEX: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(\d+)\.(\d+)(?:\.(\d+))?(?:pre\d*)?").unwrap());

/// Normalizes a version string to be compatible with semver parsing.
///
/// This function handles, or at least tries to handle, various Nix
/// vendors' complex version formats by extracting just the semantic
/// version part.
///
/// Examples of supported formats:
/// - "2.25.0-pre" -> "2.25.0"
/// - "2.24.14-1" -> "2.24.14"
/// - "`2.30pre20250521_76a4d4c2`" -> "2.30.0"
/// - "2.91.1" -> "2.91.1"
///
/// # Arguments
///
/// * `version` - The raw version string to normalize
///
/// # Returns
///
/// * `String` - The normalized version string suitable for semver parsing
pub fn normalize_version_string(version: &str) -> String {
  if let Some(captures) = VERSION_REGEX.captures(version) {
    let major = captures.get(1).map(|m| m.as_str()).unwrap_or_else(|| {
      debug!("Failed to extract major version from '{}'", version);
      version
    });
    let minor = captures.get(2).map(|m| m.as_str()).unwrap_or_else(|| {
      debug!("Failed to extract minor version from '{}'", version);
      version
    });
    let patch = captures.get(3).map_or("0", |m| m.as_str());

    let normalized = format!("{major}.{minor}.{patch}");
    if version != normalized {
      debug!("Version normalized: '{}' -> '{}'", version, normalized);
    }

    return normalized;
  }

  // Fallback: split on common separators and take the first part
  let base_version = version
    .split(&['-', '+', 'p', '_'][..])
    .next()
    .unwrap_or(version);

  // Version should have all three components (major.minor.patch)
  let normalized = match base_version.split('.').collect::<Vec<_>>().as_slice()
  {
    [major] => format!("{major}.0.0"),
    [major, minor] => format!("{major}.{minor}.0"),
    _ => base_version.to_string(),
  };

  if version != normalized {
    debug!("Version normalized: '{}' -> '{}'", version, normalized);
  }

  normalized
}

/// Retrieves the installed Nix version as a string.
///
/// This function executes the `nix --version` command, parses the output to
/// extract the version string, and returns it. This function does not perform
/// any kind of validation; it's sole purpose is to get the version. To validate
/// a version string, use `normalize_version_string()`.
///
/// # Returns
///
/// * `Result<String>` - The Nix version string or an error if the version
///   cannot be retrieved.
pub fn get_nix_version() -> Result<String> {
  let output = Command::new("nix")
    .arg("--version")
    .run_capture()?
    .ok_or_else(|| eyre::eyre!("No output from command"))?;

  let version_str = output
    .lines()
    .next()
    .ok_or_else(|| eyre::eyre!("No version string found"))?;

  Ok(version_str.to_string())
}

/// Prompts the user for ssh key login if needed
pub fn ensure_ssh_key_login() -> Result<()> {
  // ssh-add -L checks if there are any currently usable ssh keys

  if StdCommand::new("ssh-add")
    .arg("-L")
    .stdout(Stdio::null())
    .status()?
    .success()
  {
    return Ok(());
  }
  StdCommand::new("ssh-add")
    .stdin(Stdio::inherit())
    .stdout(Stdio::inherit())
    .stderr(Stdio::inherit())
    .spawn()?
    .wait()?;
  Ok(())
}

/// Gets the hostname of the current system
///
/// # Returns
///
/// * `Result<String>` - The hostname as a string or an error
pub fn get_hostname() -> Result<String> {
  #[cfg(not(target_os = "macos"))]
  {
    use color_eyre::eyre::Context;
    Ok(
      hostname::get()
        .context("Failed to get hostname")?
        .to_str()
        .map_or_else(
          || String::from("unknown-hostname"),
          std::string::ToString::to_string,
        ),
    )
  }
  #[cfg(target_os = "macos")]
  {
    use color_eyre::eyre::bail;
    use system_configuration::{
      core_foundation::{base::TCFType, string::CFString},
      sys::dynamic_store_copy_specific::SCDynamicStoreCopyLocalHostName,
    };

    let ptr = unsafe { SCDynamicStoreCopyLocalHostName(std::ptr::null()) };
    if ptr.is_null() {
      bail!("Failed to get hostname");
    }
    let name = unsafe { CFString::wrap_under_get_rule(ptr) };

    Ok(name.to_string())
  }
}

/// Resolves the hostname, preferring the supplied hostname, otherwise falling
/// back to the system's hostname.
///
/// # Arguments
/// * `supplied_hostname` - An optional hostname provided by the user.
///
/// # Returns
/// * `Ok(String)` with the resolved hostname.
/// * `Err` if no hostname is supplied and fetching the system hostname fails.
pub fn get_resolved_hostname(
  supplied_hostname: Option<String>,
) -> Result<String> {
  use color_eyre::eyre::Context;
  if let Some(h) = supplied_hostname {
    Ok(h)
  } else {
    get_hostname()
      .context("Unable to fetch hostname, and no hostname supplied.")
  }
}

/// Retrieves all enabled experimental features in Nix.
///
/// This function executes the `nix config show experimental-features` command
/// and returns a `HashSet` of the enabled features.
///
/// # Returns
///
/// * `Result<HashSet<String>>` - A `HashSet` of enabled experimental features
///   or an error.
pub fn get_nix_experimental_features() -> Result<HashSet<String>> {
  let output = Command::new("nix")
    .args(["config", "show", "experimental-features"])
    .run_capture()?;

  // If running with dry=true, output might be None
  let output_str = match output {
    Some(output) => output,
    None => return Ok(HashSet::new()),
  };

  let enabled_features: HashSet<String> =
    output_str.split_whitespace().map(String::from).collect();

  Ok(enabled_features)
}

/// Gets the missing experimental features from a required list.
///
/// # Arguments
///
/// * `required_features` - A slice of string slices representing the features
///   required.
///
/// # Returns
///
/// * `Result<Vec<String>>` - A vector of missing experimental features or an
///   error.
pub fn get_missing_experimental_features(
  required_features: &[&str],
) -> Result<Vec<String>> {
  let enabled_features = get_nix_experimental_features()?;

  let missing_features: Vec<String> = required_features
    .iter()
    .filter(|&feature| !enabled_features.contains(*feature))
    .map(|&s| s.to_string())
    .collect();

  Ok(missing_features)
}

/// Self-elevates the current process by re-executing it with sudo
///
/// # Panics
///
/// Panics if the process re-execution with elevated privileges fails.
///
/// # Examples
///
/// ```rust
/// use nh::commands::ElevationStrategy;
///
/// // Elevate the current process to run as root
/// let elevate: fn(ElevationStrategy) -> ! = nh::util::self_elevate;
/// ```
pub fn self_elevate(strategy: ElevationStrategy) -> ! {
  use std::os::unix::process::CommandExt;

  let mut cmd = crate::commands::Command::self_elevate_cmd(strategy)
    .expect("Failed to create self-elevation command");
  debug!("{:?}", cmd);
  let err = cmd.exec();
  panic!("{}", err);
}

/// Prints the difference between two generations in terms of paths and closure
/// sizes.
///
/// # Arguments
///
/// * `old_generation` - A reference to the path of the old generation.
/// * `new_generation` - A reference to the path of the new generation.
///
/// # Returns
///
/// Returns `Ok(())` if the operation completed successfully, or an error
/// wrapped in `eyre::Result` if something went wrong.
///
/// # Errors
///
/// Returns an error if the closure size thread panics or if writing size
/// differences fails.
pub fn print_dix_diff(
  old_generation: &Path,
  new_generation: &Path,
) -> Result<()> {
  let mut out = WriteFmt(io::stdout());

  // Handle to the thread collecting closure size information.
  let closure_size_handle = dix::spawn_size_diff(
    old_generation.to_path_buf(),
    new_generation.to_path_buf(),
  );

  let wrote = dix::write_paths_diffln(&mut out, old_generation, new_generation)
    .unwrap_or_default();

  if let Ok((size_old, size_new)) =
    closure_size_handle.join().map_err(|_| {
      eyre::eyre!("Failed to join closure size computation thread")
    })?
  {
    if size_old == size_new {
      info!("No version or size changes.");
    } else {
      if wrote > 0 {
        println!();
      }
      dix::write_size_diffln(&mut out, size_old, size_new)?;
    }
  }
  Ok(())
}

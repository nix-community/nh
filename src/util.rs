use std::{
    collections::HashSet, ffi::OsString, fmt, io::{self}, path::{Path, PathBuf}, process::{Command as StdCommand, Stdio}, str, sync::OnceLock
};

use color_eyre::Result;
use color_eyre::eyre;
use tempfile::TempDir;
use tracing::debug;
use which::which;

use crate::commands::Command;

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
pub fn get_nix_variant() -> Result<&'static NixVariant> {
    NIX_VARIANT.get_or_init(|| {
        let output = Command::new("nix")
            .arg("--version")
            .run_capture()
            .ok()
            .flatten();

        // XXX: If running with dry=true or Nix is not installed, output might be None
        // The latter is less likely to occur, but we still want graceful handling.
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

    Ok(NIX_VARIANT.get().unwrap())
}

/// Retrieves the installed Nix version as a string.
///
/// This function executes the `nix --version` command, parses the output to
/// extract the version string, and returns it. If the version string cannot be
/// found or parsed, it returns an error.
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

    // Extract the version substring using a regular expression
    let re = regex::Regex::new(r"\d+\.\d+\.\d+")?;
    if let Some(captures) = re.captures(version_str) {
        let version = captures
            .get(0)
            .ok_or_else(|| eyre::eyre!("No version match found"))?
            .as_str();
        return Ok(version.to_string());
    }

    Err(eyre::eyre!("Failed to extract version"))
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

/// Represents an object that may be a temporary path
pub trait MaybeTempPath: std::fmt::Debug {
    fn get_path(&self) -> &Path;
}

impl MaybeTempPath for PathBuf {
    fn get_path(&self) -> &Path {
        self.as_ref()
    }
}

impl MaybeTempPath for (PathBuf, TempDir) {
    fn get_path(&self) -> &Path {
        self.0.as_ref()
    }
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
        Ok(hostname::get()
            .context("Failed to get hostname")?
            .to_str()
            .unwrap()
            .to_string())
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
pub fn get_missing_experimental_features(required_features: &[&str]) -> Result<Vec<String>> {
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
/// // Elevate the current process to run as root
/// let elevate: fn() -> ! = nh::util::self_elevate;
/// ```
pub fn self_elevate() -> ! {
    use std::os::unix::process::CommandExt;

    let mut cmd = crate::commands::Command::self_elevate_cmd();
    debug!("{:?}", cmd);
    let err = cmd.exec();
    panic!("{}", err);
}

/// Prints the difference between two generations in terms of paths and closure sizes.
///
/// # Arguments
///
/// * `old_generation` - A reference to the path of the old generation.
/// * `new_generation` - A reference to the path of the new generation.
///
/// # Returns
///
/// Returns `Ok(())` if the operation completed successfully, or an error wrapped in `eyre::Result` if something went wrong.
///
/// # Errors
///
/// Returns an error if the closure size thread panics or if writing size differences fails.
pub fn print_dix_diff(old_generation: &Path, new_generation: &Path) -> Result<()> {
    let mut out = WriteFmt(io::stdout());

    // Handle to the thread collecting closure size information.
    let closure_size_handle =
        dix::spawn_size_diff(old_generation.to_path_buf(), new_generation.to_path_buf());

    let wrote =
        dix::write_paths_diffln(&mut out, old_generation, new_generation).unwrap_or_default();

    if let Ok((size_old, size_new)) = closure_size_handle
        .join()
        .map_err(|_| eyre::eyre!("Failed to join closure size computation thread"))?
    {
        if size_old == size_new && wrote == 0 {
            println!("No version or size changes");
        } else {
            println!();
            dix::write_size_diffln(&mut out, size_old, size_new)?;
        }
    }
    Ok(())
}

/// Gets a path to a previlege elevation program based on what is available in the system.
///
/// This funtion checks for the existence of common privilege elevation program names in
/// the `PATH` using the `which` crate and returns a Ok result with the `OsString` of the
/// path to the binary. In the case none of the checked programs are found a Err result is returned.
///
/// The search is done in this order:
///
/// 1. `doas`
/// 1. `sudo`
/// 1. `pkexec`
///
/// The logic for choosing this order is that a person with doas installed is more likely to be
/// using it as their main privilege elevation program.
///
/// # Returns
///
/// * `Result<OsString>` - The absolute path to the privilege elevation program binary or an error if a
/// program can't be found.
pub fn get_elevation_program() -> Result<OsString> {
    let strategies = ["doas", "sudo", "pkexec"];

    for strategy in strategies {
        if let Ok(path) = which(strategy) {
            debug!(?path, "{strategy} path found");
            return Ok(path.into_os_string());
        }
    }

    Err(eyre::eyre!("No elevation strategy found"))
}

use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};

use color_eyre::{Result, eyre};
use tempfile::TempDir;

use crate::commands::Command;

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

/// Determines if the Nix binary is actually Lix
///
/// # Returns
///
/// * `Result<bool>` - True if the binary is Lix, false if it's standard Nix
pub fn is_lix() -> Result<bool> {
    let output = Command::new("nix")
        .arg("--version")
        .run_capture()?
        .ok_or_else(|| eyre::eyre!("No output from command"))?;

    Ok(output.to_lowercase().contains("lix"))
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

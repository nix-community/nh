extern crate semver;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::str;

use color_eyre::{eyre, Result};
use semver::Version;
use tempfile::TempDir;

/// Compares two semantic versions and returns their order.
///
/// This function takes two version strings, parses them into `semver::Version` objects, and compares them.
/// It returns an `Ordering` indicating whether the current version is less than, equal to, or
/// greater than the target version.
///
/// # Arguments
///
/// * `current` - A string slice representing the current version.
/// * `target` - A string slice representing the target version to compare against.
///
/// # Returns
///
/// * `Result<std::cmp::Ordering>` - The comparison result.
pub fn compare_semver(current: &str, target: &str) -> Result<std::cmp::Ordering> {
    let current = Version::parse(current)?;
    let target = Version::parse(target)?;

    Ok(current.cmp(&target))
}

/// Retrieves the installed Nix version as a string.
///
/// This function executes the `nix --version` command, parses the output to extract the version string,
/// and returns it. If the version string cannot be found or parsed, it returns an error.
///
/// # Returns
///
/// * `Result<String>` - The Nix version string or an error if the version cannot be retrieved.
pub fn get_nix_version() -> Result<String> {
    let output = Command::new("nix").arg("--version").output()?;

    let output_str = str::from_utf8(&output.stdout)?;
    let version_str = output_str
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

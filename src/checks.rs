use std::{cmp::Ordering, env};

use color_eyre::{eyre, Result};
use semver::Version;
use tracing::warn;

use crate::util;

/// Verifies if the installed Nix version meets requirements
///
/// # Returns
///
/// * `Result<()>` - Ok if version requirements are met, error otherwise
pub fn check_nix_version() -> Result<()> {
    if env::var("NH_NO_CHECKS").is_ok() {
        return Ok(());
    }

    let version = util::get_nix_version()?;
    let is_lix_binary = util::is_lix()?;

    // XXX: Both Nix and Lix follow semantic versioning (semver). Update the
    // versions below once latest stable for either of those packages change.
    // TODO: Set up a CI to automatically update those in the future.
    const MIN_LIX_VERSION: &str = "2.91.1";
    const MIN_NIX_VERSION: &str = "2.24.14";

    // Minimum supported versions. Those should generally correspond to
    // latest package versions in the stable branch.
    //
    // Q: Why are you doing this?
    // A: First of all to make sure we do not make baseless assumptions
    // about the user's system; we should only work around APIs that we
    // are fully aware of, and not try to work around every edge case.
    // Also, nh should be responsible for nudging the user to use the
    // relevant versions of the software it wraps, so that we do not have
    // to try and support too many versions. NixOS stable and unstable
    // will ALWAYS be supported, but outdated versions will not. If your
    // Nix fork uses a different versioning scheme, please open an issue.
    let min_version = if is_lix_binary {
        MIN_LIX_VERSION
    } else {
        MIN_NIX_VERSION
    };

    let current = Version::parse(&version)?;
    let required = Version::parse(min_version)?;

    match current.cmp(&required) {
        Ordering::Less => {
            let binary_name = if is_lix_binary { "Lix" } else { "Nix" };
            warn!(
                "Warning: {} version {} is older than the recommended minimum version {}. You may encounter issues.",
                binary_name,
                version,
                min_version
            );
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Verifies if the required experimental features are enabled
///
/// # Returns
///
/// * `Result<()>` - Ok if all required features are enabled, error otherwise
pub fn check_nix_features() -> Result<()> {
    if env::var("NH_NO_CHECKS").is_ok() {
        return Ok(());
    }

    let mut required_features = vec!["nix-command", "flakes"];

    // Lix up until 2.93.0 uses repl-flake, which is removed in the latest version of Nix.
    if util::is_lix()? {
        let repl_flake_removed_in_lix_version = Version::parse("2.93.0")?;
        let current_lix_version = Version::parse(&util::get_nix_version()?)?;
        if current_lix_version < repl_flake_removed_in_lix_version {
            required_features.push("repl-flake");
        }
    }

    tracing::debug!("Required Nix features: {}", required_features.join(", "));

    // Get currently enabled features
    match util::get_nix_experimental_features() {
        Ok(enabled_features) => {
            let features_vec: Vec<_> = enabled_features.into_iter().collect();
            tracing::debug!("Enabled Nix features: {}", features_vec.join(", "));
        }
        Err(e) => {
            tracing::warn!("Failed to get enabled Nix features: {}", e);
        }
    }

    let missing_features = util::get_missing_experimental_features(&required_features)?;

    if !missing_features.is_empty() {
        tracing::warn!(
            "Missing required Nix features: {}",
            missing_features.join(", ")
        );
        return Err(eyre::eyre!(
            "Missing required experimental features. Please enable: {}",
            missing_features.join(", ")
        ));
    }

    tracing::debug!("All required Nix features are enabled");
    Ok(())
}

/// Handles environment variable setup and returns if a warning should be shown
///
/// # Returns
///
/// * `Result<bool>` - True if a warning should be shown about the FLAKE
///   variable, false otherwise
pub fn setup_environment() -> Result<bool> {
    let mut do_warn = false;

    if let Ok(f) = std::env::var("FLAKE") {
        // Set NH_FLAKE if it's not already set
        if std::env::var("NH_FLAKE").is_err() {
            unsafe { std::env::set_var("NH_FLAKE", f); }

            // Only warn if FLAKE is set and we're using it to set NH_FLAKE
            // AND none of the command-specific env vars are set
            if std::env::var("NH_OS_FLAKE").is_err()
                && std::env::var("NH_HOME_FLAKE").is_err()
                && std::env::var("NH_DARWIN_FLAKE").is_err()
            {
                do_warn = true;
            }
        }
    }

    Ok(do_warn)
}

/// Consolidate all necessary checks for Nix functionality into a single
/// function. This will be executed in the main function, but can be executed
/// before critical commands to double-check if necessary.
///
/// # Returns
///
/// * `Result<()>` - Ok if all checks pass, error otherwise
pub fn verify_nix_environment() -> Result<()> {
    if env::var("NH_NO_CHECKS").is_ok() {
        return Ok(());
    }

    check_nix_version()?;
    check_nix_features()?;
    Ok(())
}

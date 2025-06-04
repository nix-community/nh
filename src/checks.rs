use std::{cmp::Ordering, env};

use color_eyre::{Result, eyre};
use semver::Version;
use tracing::{info, warn};

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
                binary_name, version, min_version
            );
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Verifies that flakes are enabled
///
/// # Returns
///
/// * `Result<()>` - Ok if flakes are enabled, error otherwise
pub fn check_flakes_enabled() -> Result<()> {
    if env::var("NH_NO_CHECKS").is_ok() {
        return Ok(());
    }

    info!("Checking that flakes are enabled");
    let flakes_enabled = std::process::Command::new("nix")
        .args(["eval", "--expr", "builtins.getFlake"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;

    if !flakes_enabled.success() {
        warn!("Flakes are not enabled");
        return Err(eyre::eyre!("Flakes are not enabled. Please enable them."));
    }

    tracing::debug!("Flakes are enabled");

    if util::is_lix()? {
        let dir = tempfile::Builder::new()
            .prefix("nh-repl-flake-feature-check")
            .tempdir()?;
        let f = dir.path().join("flake.nix");
        std::fs::write(&f, "{ outputs = _: {}; }")?;

        info!("Checking that the repl-flake feature is enabled");
        let repl_flake_enabled = std::process::Command::new("nix")
            .current_dir(dir.path().to_path_buf())
            .arg("repl")
            .arg(dir.path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;

        if !repl_flake_enabled.success() {
            warn!("The repl-flake feature is not enabled");
            return Err(eyre::eyre!(
                "The repl-flake feature is not enabled. Please enable it."
            ));
        }

        tracing::debug!("The repl-flake feature is enabled");

        let _ = dir.close();
    }

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
            unsafe {
                std::env::set_var("NH_FLAKE", f);
            }

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
    check_flakes_enabled()?;
    Ok(())
}

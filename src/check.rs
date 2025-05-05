use std::{
    cmp::Ordering,
    env,
    sync::Mutex,
};

use color_eyre::{
    eyre,
    Result,
};
use semver::Version;

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
            Err(eyre::eyre!(
                "{} version {} is too old. Minimum required version is {}",
                binary_name,
                version,
                min_version
            ))
        },
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

    // Lix still uses repl-flake, which is removed in the latest version of Nix.
    if util::is_lix()? {
        required_features.push("repl-flake");
    }

    tracing::debug!("Required Nix features: {}", required_features.join(", "));

    // Get currently enabled features
    match util::get_nix_experimental_features() {
        Ok(enabled_features) => {
            let features_vec: Vec<_> = enabled_features.into_iter().collect();
            tracing::debug!("Enabled Nix features: {}", features_vec.join(", "));
        },
        Err(e) => {
            tracing::warn!("Failed to get enabled Nix features: {}", e);
        },
    }

    let missing_features = util::get_missing_experimental_features(&required_features)?;

    if !missing_features.is_empty() {
        tracing::warn!("Missing required Nix features: {}", missing_features.join(", "));
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
            std::env::set_var("NH_FLAKE", f);

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

// Environment variables are global state, so tests need to be run sequentially.
// Using a mutex to ensure that env var manipulation in one test doesn't affect
// others. Alternatively, run tests with `cargo test -- --test-threads=1`
#[allow(dead_code)] // suppress 'false' positives
static ENV_LOCK: Mutex<()> = Mutex::new(());

// Clean up environment variables set during tests
#[allow(dead_code)]
fn cleanup_env_vars() {
    env::remove_var("FLAKE");
    env::remove_var("NH_FLAKE");
    env::remove_var("NH_OS_FLAKE");
    env::remove_var("NH_HOME_FLAKE");
    env::remove_var("NH_DARWIN_FLAKE");
    env::remove_var("NH_NO_CHECKS");
}

#[test]
fn test_setup_environment_no_flake_set() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    cleanup_env_vars();

    let should_warn = setup_environment()?;
    assert!(!should_warn, "Should not warn when FLAKE is not set");
    assert!(env::var("NH_FLAKE").is_err(), "NH_FLAKE should not be set");

    cleanup_env_vars();
    Ok(())
}

#[test]
fn test_setup_environment_flake_set_no_nh_flake_no_specifics() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    cleanup_env_vars();

    env::set_var("FLAKE", "test_flake_path");
    let should_warn = setup_environment()?;

    assert!(
        should_warn,
        "Should warn when FLAKE is set, NH_FLAKE is not, and no specific NH_*_FLAKE vars are set"
    );
    assert_eq!(
        env::var("NH_FLAKE").unwrap(),
        "test_flake_path",
        "NH_FLAKE should be set from FLAKE"
    );

    cleanup_env_vars();
    Ok(())
}

#[test]
fn test_setup_environment_flake_set_nh_flake_already_set() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    cleanup_env_vars();

    env::set_var("FLAKE", "test_flake_path");
    env::set_var("NH_FLAKE", "existing_nh_flake_path");
    let should_warn = setup_environment()?;

    assert!(!should_warn, "Should not warn when NH_FLAKE is already set");
    assert_eq!(
        env::var("NH_FLAKE").unwrap(),
        "existing_nh_flake_path",
        "NH_FLAKE should retain its original value"
    );

    cleanup_env_vars();
    Ok(())
}

#[test]
fn test_setup_environment_flake_set_no_nh_flake_nh_os_flake_set() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    cleanup_env_vars();

    env::set_var("FLAKE", "test_flake_path");
    env::set_var("NH_OS_FLAKE", "os_specific_flake");
    let should_warn = setup_environment()?;

    assert!(
        !should_warn,
        "Should not warn when FLAKE is set, NH_FLAKE is not, but NH_OS_FLAKE is set"
    );
    assert_eq!(
        env::var("NH_FLAKE").unwrap(),
        "test_flake_path",
        "NH_FLAKE should be set from FLAKE"
    );
    assert_eq!(
        env::var("NH_OS_FLAKE").unwrap(),
        "os_specific_flake",
        "NH_OS_FLAKE should remain set"
    );

    cleanup_env_vars();
    Ok(())
}

#[test]
fn test_setup_environment_flake_set_no_nh_flake_nh_home_flake_set() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    cleanup_env_vars();

    env::set_var("FLAKE", "test_flake_path");
    env::set_var("NH_HOME_FLAKE", "home_specific_flake");
    let should_warn = setup_environment()?;

    assert!(
        !should_warn,
        "Should not warn when FLAKE is set, NH_FLAKE is not, but NH_HOME_FLAKE is set"
    );
    assert_eq!(
        env::var("NH_FLAKE").unwrap(),
        "test_flake_path",
        "NH_FLAKE should be set from FLAKE"
    );
    assert_eq!(
        env::var("NH_HOME_FLAKE").unwrap(),
        "home_specific_flake",
        "NH_HOME_FLAKE should remain set"
    );

    cleanup_env_vars();
    Ok(())
}

#[test]
// Greatest function name ever.
// testSetupEnvironmentFlakeSetNoNhFlakeNhDarwinFlakeSetAbstractFactoryBuilder
fn test_setup_environment_flake_set_no_nh_flake_nh_darwin_flake_set() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    cleanup_env_vars();

    env::set_var("FLAKE", "test_flake_path");
    env::set_var("NH_DARWIN_FLAKE", "darwin_specific_flake");
    let should_warn = setup_environment()?;

    assert!(
        !should_warn,
        "Should not warn when FLAKE is set, NH_FLAKE is not, but NH_DARWIN_FLAKE is set"
    );
    assert_eq!(
        env::var("NH_FLAKE").unwrap(),
        "test_flake_path",
        "NH_FLAKE should be set from FLAKE"
    );
    assert_eq!(
        env::var("NH_DARWIN_FLAKE").unwrap(),
        "darwin_specific_flake",
        "NH_DARWIN_FLAKE should remain set"
    );

    cleanup_env_vars(); // Clean up after test
    Ok(())
}

#[test]
fn test_checks_skip_when_no_checks_set() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    cleanup_env_vars();

    env::set_var("NH_NO_CHECKS", "1");

    // These should succeed even with invalid environment
    let version_check = check_nix_version();
    let features_check = check_nix_features();
    let verify_check = verify_nix_environment();

    assert!(version_check.is_ok(), "Version check should be skipped");
    assert!(features_check.is_ok(), "Features check should be skipped");
    assert!(verify_check.is_ok(), "Verify check should be skipped");

    cleanup_env_vars();
    Ok(())
}

use std::sync::LazyLock;
use std::{cmp::Ordering, env};

use color_eyre::Result;
use regex::Regex;
use semver::Version;
use tracing::{debug, warn};

use crate::util::{self, NixVariant};

// Static regex compiled once for version string normalization
static VERSION_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+)\.(\d+)(?:\.(\d+))?").unwrap());

/// Normalizes a version string to be compatible with semver parsing.
///
/// This function handles Nix's complex version formats by extracting just the
/// semantic version part. Examples of supported formats:
/// - "2.25.0-pre" -> "2.25.0"
/// - "2.24.14-1" -> "2.24.14"
/// - "2.30pre20250521_76a4d4c2" -> "2.30.0"
/// - "2.91.1" -> "2.91.1"
///
/// # Arguments
///
/// * `version` - The raw version string to normalize
///
/// # Returns
///
/// * `String` - The normalized version string suitable for semver parsing
fn normalize_version_string(version: &str) -> String {
    // First, try to extract a version pattern like X.Y or X.Y.Z from the beginning
    if let Some(captures) = VERSION_REGEX.captures(version) {
        let major = captures.get(1).unwrap().as_str();
        let minor = captures.get(2).unwrap().as_str();
        let patch = captures.get(3).map(|m| m.as_str()).unwrap_or("0");

        format!("{}.{}.{}", major, minor, patch)
    } else {
        // Fallback: split on common separators and take the first part
        let base_version = version
            .split(&['-', '+', 'p', '_'][..])
            .next()
            .unwrap_or(version);

        // Version should have all three components (major.minor.patch)
        let parts: Vec<&str> = base_version.split('.').collect();
        match parts.len() {
            1 => format!("{}.0.0", parts[0]),            // "1" -> "1.0.0"
            2 => format!("{}.{}.0", parts[0], parts[1]), // "1.2" -> "1.2.0"
            _ => base_version.to_string(),               // "1.2.3" or more parts, use as-is
        }
    }
}

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
    let nix_variant = util::get_nix_variant()?;

    // XXX: Both Nix and Lix follow semantic versioning (semver). Update the
    // versions below once latest stable for either of those packages change.
    // We *also* cannot (or rather, will not) make this check for non-nixpkgs
    // Nix variants, since there is no good baseline for what to support
    // without the understanding of stable/unstable branches. What do we check
    // for, whether upstream made an announcement? No thanks.
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
    let min_version = match nix_variant {
        util::NixVariant::Lix => MIN_LIX_VERSION,
        _ => MIN_NIX_VERSION,
    };

    // Normalize the version string to handle pre-release versions and distro suffixes
    let normalized_version = normalize_version_string(&version);

    let current = match Version::parse(&normalized_version) {
        Ok(ver) => ver,
        Err(e) => {
            warn!(
                "Failed to parse Nix version '{}' (normalized: '{}'): {}. Skipping version check.",
                version, normalized_version, e
            );
            return Ok(());
        }
    };

    let required = Version::parse(min_version)?;

    match current.cmp(&required) {
        Ordering::Less => {
            let binary_name = match nix_variant {
                util::NixVariant::Lix => "Lix",
                util::NixVariant::Determinate => "Determinate Nix",
                util::NixVariant::Nix => "Nix",
            };
            warn!(
                "Warning: {} version {} is older than the recommended minimum version {}. You may encounter issues.",
                binary_name, version, min_version
            );
            Ok(())
        }
        _ => Ok(()),
    }
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
/// NOTE: Experimental feature checks are now done per-command to avoid
/// redundant error messages for features not needed by the specific command.
///
/// # Returns
///
/// * `Result<()>` - Ok if all checks pass, error otherwise
pub fn verify_nix_environment() -> Result<()> {
    if env::var("NH_NO_CHECKS").is_ok() {
        return Ok(());
    }

    // Only check version globally. Features are checked per-command now.
    // This function is kept as is for backwards compatibility.
    check_nix_version()?;
    Ok(())
}

/// Trait for types that have feature requirements
pub trait FeatureRequirements {
    /// Returns the list of required experimental features
    fn required_features(&self) -> Vec<&'static str>;

    /// Checks if all required features are enabled
    fn check_features(&self) -> Result<()> {
        if env::var("NH_NO_CHECKS").is_ok() {
            return Ok(());
        }

        let required = self.required_features();
        if required.is_empty() {
            return Ok(());
        }

        debug!("Required Nix features: {}", required.join(", "));

        let missing = util::get_missing_experimental_features(&required)?;
        if !missing.is_empty() {
            return Err(color_eyre::eyre::eyre!(
                "Missing required experimental features for this command: {}",
                missing.join(", ")
            ));
        }

        debug!("All required Nix features are enabled");
        Ok(())
    }
}

/// Feature requirements for commands that use flakes
#[derive(Debug)]
pub struct FlakeFeatures;

impl FeatureRequirements for FlakeFeatures {
    fn required_features(&self) -> Vec<&'static str> {
        let mut features = vec![];

        // Determinate Nix doesn't require nix-command or flakes to be experimental
        // as they simply decided to mark those as no-longer-experimental-lol. Remove
        // redundant experimental features if the Nix variant is determinate.
        if let Ok(variant) = util::get_nix_variant() {
            if !matches!(variant, NixVariant::Determinate) {
                features.push("nix-command");
                features.push("flakes");
            }
        }

        features
    }
}

/// Feature requirements for legacy (non-flake) commands
/// XXX: There are actually no experimental feature requirements for legacy (nix2) CLI
/// but since move-fast-break-everything is a common mantra among Nix & Nix-adjecent
/// software, I've implemented this. Do not remove, this is simply for futureproofing.
#[derive(Debug)]
pub struct LegacyFeatures;

impl FeatureRequirements for LegacyFeatures {
    fn required_features(&self) -> Vec<&'static str> {
        vec![]
    }
}

/// Feature requirements for OS repl commands
#[derive(Debug)]
pub struct OsReplFeatures {
    pub is_flake: bool,
}

impl FeatureRequirements for OsReplFeatures {
    fn required_features(&self) -> Vec<&'static str> {
        let mut features = vec![];

        // For non-flake repls, no experimental features needed
        if !self.is_flake {
            return features;
        }

        // For flake repls, check if we need experimental features
        if let Ok(variant) = util::get_nix_variant() {
            match variant {
                NixVariant::Determinate => {
                    // Determinate Nix doesn't need experimental features
                }
                NixVariant::Lix => {
                    features.push("nix-command");
                    features.push("flakes");

                    // Lix-specific repl-flake feature for older versions
                    if let Ok(version) = util::get_nix_version() {
                        let normalized_version = normalize_version_string(&version);
                        if let Ok(current) = Version::parse(&normalized_version) {
                            if let Ok(threshold) = Version::parse("2.93.0") {
                                if current < threshold {
                                    features.push("repl-flake");
                                }
                            }
                        }
                    }
                }
                NixVariant::Nix => {
                    features.push("nix-command");
                    features.push("flakes");
                }
            }
        }

        features
    }
}

/// Feature requirements for Home Manager repl commands
#[derive(Debug)]
pub struct HomeReplFeatures {
    pub is_flake: bool,
}

impl FeatureRequirements for HomeReplFeatures {
    fn required_features(&self) -> Vec<&'static str> {
        let mut features = vec![];

        // For non-flake repls, no experimental features needed
        if !self.is_flake {
            return features;
        }

        // For flake repls, only need nix-command and flakes
        if let Ok(variant) = util::get_nix_variant() {
            if !matches!(variant, NixVariant::Determinate) {
                features.push("nix-command");
                features.push("flakes");
            }
        }

        features
    }
}

/// Feature requirements for Darwin repl commands
#[derive(Debug)]
pub struct DarwinReplFeatures {
    pub is_flake: bool,
}

impl FeatureRequirements for DarwinReplFeatures {
    fn required_features(&self) -> Vec<&'static str> {
        let mut features = vec![];

        // For non-flake repls, no experimental features needed
        if !self.is_flake {
            return features;
        }

        // For flake repls, only need nix-command and flakes
        if let Ok(variant) = util::get_nix_variant() {
            if !matches!(variant, NixVariant::Determinate) {
                features.push("nix-command");
                features.push("flakes");
            }
        }

        features
    }
}

/// Feature requirements for commands that don't need experimental features
#[derive(Debug)]
pub struct NoFeatures;

impl FeatureRequirements for NoFeatures {
    fn required_features(&self) -> Vec<&'static str> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use proptest::prelude::*;
    use serial_test::serial;

    use super::*;

    // This helps set environment variables safely in tests
    struct EnvGuard {
        key: String,
        original: Option<String>,
    }

    impl EnvGuard {
        fn new(key: &str, value: &str) -> Self {
            let original = env::var(key).ok();
            unsafe {
                env::set_var(key, value);
            }
            EnvGuard {
                key: key.to_string(),
                original,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.original {
                    Some(val) => env::set_var(&self.key, val),
                    None => env::remove_var(&self.key),
                }
            }
        }
    }

    proptest! {
        #[test]
        fn test_normalize_version_string_handles_various_formats(
            major in 1u32..10,
            minor in 0u32..99,
            patch in 0u32..99
        ) {
            // Test basic semver format
            let basic = format!("{}.{}.{}", major, minor, patch);
            prop_assert_eq!(normalize_version_string(&basic), basic.clone());

            // Test with pre-release suffix
            let pre_release = format!("{}.{}.{}-pre", major, minor, patch);
            prop_assert_eq!(normalize_version_string(&pre_release), basic.clone());

            // Test with distro suffix
            let distro = format!("{}.{}.{}-1", major, minor, patch);
            prop_assert_eq!(normalize_version_string(&distro), basic.clone());

            // Test Nix-style version without patch (should add .0)
            let no_patch = format!("{}.{}", major, minor);
            let expected_no_patch = format!("{}.{}.0", major, minor);
            prop_assert_eq!(normalize_version_string(&no_patch), expected_no_patch);

            // Test complex Nix format like "2.30pre20250521_76a4d4c2"
            let complex = format!("{}.{}pre20250521_76a4d4c2", major, minor);
            let expected_complex = format!("{}.{}.0", major, minor);
            prop_assert_eq!(normalize_version_string(&complex), expected_complex);
        }

        #[test]
        fn test_flake_features_always_returns_consistent_results(
            _dummy in 0..100u32
        ) {
            let features = FlakeFeatures;
            let result1 = features.required_features();
            let result2 = features.required_features();

            // Property: Multiple calls should return identical results
            prop_assert_eq!(result1.clone(), result2.clone());

            // Property: Should only contain known experimental features
            for feature in &result1 {
                prop_assert!(
                    *feature == "nix-command" ||
                    *feature == "flakes",
                    "Unknown feature: {}", feature
                );
            }

            // Property: Results should be deterministic based on variant
            // We can't control the actual variant in this test, but we can verify
            // that the logic is consistent
            if result1.is_empty() {
                // If empty, variant should be Determinate (when available)
                // This property holds when the system has Determinate Nix
            } else {
                // If not empty, should contain both nix-command and flakes
                prop_assert!(result1.contains(&"nix-command"));
                prop_assert!(result1.contains(&"flakes"));
                prop_assert_eq!(result1.len(), 2);
            }
        }

        #[test]
        fn test_legacy_features_always_empty(
            _dummy in 0..100u32
        ) {
            let features = LegacyFeatures;
            let result = features.required_features();

            // Property: Legacy features should always be empty
            prop_assert!(result.is_empty());
        }

        #[test]
        fn test_no_features_always_empty(
            _dummy in 0..100u32
        ) {
            let features = NoFeatures;
            let result = features.required_features();

            // Property: NoFeatures should always be empty
            prop_assert!(result.is_empty());
        }

        #[test]
        fn test_repl_features_consistency_with_flake_flag(
            is_flake in any::<bool>()
        ) {
            // Test OS repl features
            let os_features = OsReplFeatures { is_flake };
            let os_result = os_features.required_features();

            // Test Home repl features
            let home_features = HomeReplFeatures { is_flake };
            let home_result = home_features.required_features();

            // Test Darwin repl features
            let darwin_features = DarwinReplFeatures { is_flake };
            let darwin_result = darwin_features.required_features();

            if !is_flake {
                // Property: Non-flake repls should never require features
                prop_assert!(os_result.is_empty());
                prop_assert!(home_result.is_empty());
                prop_assert!(darwin_result.is_empty());
            } else {
                // Property: All flake repls should have consistent base features
                // (when features are required, they should include nix-command and flakes)
                for result in [&os_result, &home_result, &darwin_result] {
                    if !result.is_empty() {
                        prop_assert!(result.contains(&"nix-command"));
                        prop_assert!(result.contains(&"flakes"));
                    }
                }

                // Property: Only OS repl may have additional features (repl-flake for older Lix)
                // Home and Darwin should never have more than the base features
                if !home_result.is_empty() {
                    prop_assert_eq!(home_result.len(), 2);
                }
                if !darwin_result.is_empty() {
                    prop_assert_eq!(darwin_result.len(), 2);
                }

                // Property: OS repl may have 2 or 3 features (base + optional repl-flake)
                if !os_result.is_empty() {
                    prop_assert!(os_result.len() >= 2 && os_result.len() <= 3);
                    if os_result.len() == 3 {
                        prop_assert!(os_result.contains(&"repl-flake"));
                    }
                }
            }
        }

        #[test]
        fn test_feature_requirements_trait_idempotency(
            is_flake in any::<bool>()
        ) {
            let test_cases = vec![
                Box::new(FlakeFeatures) as Box<dyn FeatureRequirements>,
                Box::new(LegacyFeatures) as Box<dyn FeatureRequirements>,
                Box::new(OsReplFeatures { is_flake }) as Box<dyn FeatureRequirements>,
                Box::new(HomeReplFeatures { is_flake }) as Box<dyn FeatureRequirements>,
                Box::new(DarwinReplFeatures { is_flake }) as Box<dyn FeatureRequirements>,
                Box::new(NoFeatures) as Box<dyn FeatureRequirements>,
            ];

            for feature_req in test_cases {
                let result1 = feature_req.required_features();
                let result2 = feature_req.required_features();

                // Property: Multiple calls should be idempotent
                prop_assert_eq!(result1.clone(), result2.clone());

                // Property: All features should be valid strings
                for feature in &result1 {
                    prop_assert!(!feature.is_empty());
                    prop_assert!(feature.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
                }

                // Property: No duplicate features
                let mut sorted = result1.clone();
                sorted.sort();
                sorted.dedup();
                prop_assert_eq!(result1.len(), sorted.len());
            }
        }
    }

    // Regular unit tests for specific scenarios
    #[test]
    fn test_normalize_version_string_with_real_nix_versions() {
        // Test the exact format you mentioned
        assert_eq!(
            normalize_version_string("2.30pre20250521_76a4d4c2"),
            "2.30.0"
        );

        // Test other real Nix version formats
        assert_eq!(normalize_version_string("2.25.0-pre"), "2.25.0");
        assert_eq!(normalize_version_string("2.24.14-1"), "2.24.14");
        assert_eq!(normalize_version_string("2.91.1"), "2.91.1");
        assert_eq!(normalize_version_string("2.18"), "2.18.0");

        // Test edge cases
        assert_eq!(normalize_version_string("3.0dev"), "3.0.0");
        assert_eq!(normalize_version_string("2.22rc1"), "2.22.0");
        assert_eq!(normalize_version_string("2.19_git_abc123"), "2.19.0");

        // Test fallback cases where patch component is missing
        assert_eq!(normalize_version_string("1.2-beta"), "1.2.0");
        assert_eq!(normalize_version_string("3.4+build.1"), "3.4.0");
        assert_eq!(normalize_version_string("5.6_alpha"), "5.6.0");

        // Test fallback cases where both minor and patch are missing
        assert_eq!(normalize_version_string("2-rc1"), "2.0.0");
        assert_eq!(normalize_version_string("4+build"), "4.0.0");
        assert_eq!(normalize_version_string("7_dev"), "7.0.0");
    }

    #[test]
    #[serial]
    fn test_setup_environment_flake_to_nh_flake_migration() {
        unsafe {
            env::remove_var("FLAKE");
            env::remove_var("NH_FLAKE");
            env::remove_var("NH_OS_FLAKE");
            env::remove_var("NH_HOME_FLAKE");
            env::remove_var("NH_DARWIN_FLAKE");
        }

        let _guard = EnvGuard::new("FLAKE", "/test/flake");

        let result = setup_environment().expect("setup_environment should succeed");

        assert!(result, "Should warn when migrating FLAKE to NH_FLAKE");
        assert_eq!(env::var("NH_FLAKE").unwrap(), "/test/flake");
    }

    #[test]
    #[serial]
    fn test_setup_environment_no_migration_when_nh_flake_exists() {
        unsafe {
            env::remove_var("FLAKE");
            env::remove_var("NH_FLAKE");
            env::remove_var("NH_OS_FLAKE");
            env::remove_var("NH_HOME_FLAKE");
            env::remove_var("NH_DARWIN_FLAKE");
        }

        let _guard1 = EnvGuard::new("FLAKE", "/test/flake");
        let _guard2 = EnvGuard::new("NH_FLAKE", "/existing/flake");

        let result = setup_environment().expect("setup_environment should succeed");

        assert!(!result, "Should not warn when NH_FLAKE already exists");
        assert_eq!(env::var("NH_FLAKE").unwrap(), "/existing/flake");
    }

    #[test]
    #[serial]
    fn test_setup_environment_no_migration_when_specific_flake_vars_exist() {
        unsafe {
            env::remove_var("FLAKE");
            env::remove_var("NH_FLAKE");
            env::remove_var("NH_OS_FLAKE");
            env::remove_var("NH_HOME_FLAKE");
            env::remove_var("NH_DARWIN_FLAKE");
        }

        let _guard1 = EnvGuard::new("FLAKE", "/test/flake");
        let _guard2 = EnvGuard::new("NH_OS_FLAKE", "/os/flake");

        let result = setup_environment().expect("setup_environment should succeed");

        assert!(!result, "Should not warn when specific flake vars exist");
        assert_eq!(env::var("NH_FLAKE").unwrap(), "/test/flake");
    }

    #[test]
    #[serial]
    fn test_check_features_bypassed_with_nh_no_checks() {
        let _guard = EnvGuard::new("NH_NO_CHECKS", "1");

        let features = FlakeFeatures;
        let result = features.check_features();

        assert!(
            result.is_ok(),
            "check_features should succeed when NH_NO_CHECKS is set"
        );
    }

    #[test]
    #[serial]
    fn test_verify_nix_environment_bypassed_with_nh_no_checks() {
        let _guard = EnvGuard::new("NH_NO_CHECKS", "1");

        let result = verify_nix_environment();

        assert!(
            result.is_ok(),
            "verify_nix_environment should succeed when NH_NO_CHECKS is set"
        );
    }

    #[test]
    #[serial]
    fn test_check_nix_version_bypassed_with_nh_no_checks() {
        let _guard = EnvGuard::new("NH_NO_CHECKS", "1");

        let result = check_nix_version();

        assert!(
            result.is_ok(),
            "check_nix_version should succeed when NH_NO_CHECKS is set"
        );
    }

    proptest! {
        #[test]
        #[serial]
        fn test_env_guard_cleanup_property(
            key in "[A-Z_]{1,20}",
            value in "[a-zA-Z0-9/._-]{1,50}"
        ) {
            let original = env::var(&key).ok();

            {
                let _guard = EnvGuard::new(&key, &value);
                prop_assert_eq!(env::var(&key).unwrap(), value);
            }

            // Property: Environment should be restored after guard is dropped
            match original {
                Some(orig_val) => prop_assert_eq!(env::var(&key).unwrap(), orig_val),
                None => prop_assert!(env::var(&key).is_err()),
            }
        }
    }
}

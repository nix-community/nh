use std::{
    path::PathBuf,
    sync::{
        LazyLock,
        Mutex,
    },
};

use proptest::{
    prelude::*,
    strategy::Strategy,
    test_runner::Config,
};

// Global mutex to prevent parallel property tests from interfering with each
// other
static PROP_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Create a default proptest configuration suitable for nh
pub fn default_config() -> proptest::test_runner::Config {
    Config {
        cases: 100,            // Number of test cases to run
        max_shrink_iters: 500, // Maximum number of shrink iterations
        ..Default::default()
    }
}

/// Run a property test with the given configuration and initialization function
pub fn with_proptest_config<F>(config: Config, init: F)
where
    F: FnOnce(),
{
    // Acquire the lock to prevent parallel test runs
    let _lock = PROP_TEST_LOCK.lock().unwrap();

    // Set the global config (just a placeholder, as we're not actually
    // directly using the config here - the proptest! macro handles that)
    let _config = config;

    init();
    // The lock is released when _lock goes out of scope
}

/// Generic strategy for valid file paths
pub fn file_path_strategy() -> impl Strategy<Value = PathBuf> {
    // Generate realistic file paths that are likely to be valid
    proptest::string::string_regex("[a-zA-Z0-9_./-]{1,50}")
        .unwrap()
        .prop_map(PathBuf::from)
}

/// Strategy for generating valid flake references
pub fn flake_reference_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Local file paths
        file_path_strategy().prop_map(|p| p.to_string_lossy().to_string()),
        // GitHub references
        "github:[a-zA-Z0-9_-]{1,20}/[a-zA-Z0-9_-]{1,20}".prop_map(String::from),
        // Direct paths
        "/[a-zA-Z0-9/_-]+".prop_map(String::from),
        // Common flake references
        Just("nixpkgs".to_string()),
        Just("home-manager".to_string()),
        Just(".".to_string())
    ]
}

/// Strategy for generating valid attribute paths
pub fn attribute_path_strategy() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(
        proptest::string::string_regex("[a-zA-Z0-9_]{1,10}").unwrap(),
        1..5,
    )
}

/// Strategy for generating valid hostname strings
pub fn hostname_strategy() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-zA-Z0-9-]{1,15}").unwrap()
}

/// Strategy for generating valid env variable names
pub fn env_var_name_strategy() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[A-Z_]{1,20}").unwrap()
}

/// Strategy for generating valid env variable values
pub fn env_var_value_strategy() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-zA-Z0-9_/.:-]{1,50}").unwrap()
}
